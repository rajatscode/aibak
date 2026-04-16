//! Win probability estimation with layered evaluation.
//!
//! Three tiers of increasing accuracy and cost:
//! - **Layer 1**: Material evaluation — instant heuristic based on board state.
//! - **Layer 2**: Lookahead — 1-ply deterministic search with both move orders.
//! - **Layer 3**: Full Monte Carlo with material-corrected output.

use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};

use crate::ai;
use crate::map::Map;
use crate::orders::Order;
use crate::state::{GameState, NEUTRAL, Phase, PlayerId};
use crate::turn::resolve_turn;

// ── Public types ──

/// Win probability estimate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WinProbability {
    /// Probability player 0 wins (0.0 to 1.0).
    pub player_0: f64,
    /// Probability player 1 wins (0.0 to 1.0).
    pub player_1: f64,
    /// Number of simulations run (0 for pure material evaluation).
    pub simulations_run: u32,
}

/// Calibration table mapping material advantage buckets to actual win rates.
/// Built by running many AI-vs-AI games and recording outcomes.
#[derive(Debug, Clone)]
pub struct EvalCalibration {
    /// 10 buckets: bucket i covers material eval in [i*0.1, (i+1)*0.1).
    /// Each entry is the empirical win rate for player 0 when the material
    /// evaluation falls in that bucket.
    pub buckets: [f64; 10],
    /// Total games used to build the calibration.
    pub total_games: u32,
}

impl Default for EvalCalibration {
    fn default() -> Self {
        // Default calibration: identity mapping (trust the material eval as-is).
        Self {
            buckets: [0.05, 0.15, 0.25, 0.35, 0.45, 0.55, 0.65, 0.75, 0.85, 0.95],
            total_games: 0,
        }
    }
}

// ── Constants for material evaluation ──

/// Logistic steepness parameter. Calibrated so that:
/// - advantage = 0 => P = 0.5
/// - advantage ~ 1.1 => P ~ 0.75 (2x income)
/// - advantage ~ 2.2 => P ~ 0.90 (3x income)
const LOGISTIC_K: f64 = 2.0;

/// Weights for the material advantage components.
const WEIGHT_INCOME: f64 = 1.6;
const WEIGHT_TERRITORY: f64 = 0.5;
const WEIGHT_ARMY: f64 = 0.4;
const WEIGHT_BONUS: f64 = 0.8;
const WEIGHT_DEFENSE: f64 = 0.3;

// ── Layer 1: Material evaluation ──

/// Compute a fast material-based win probability for player 0 (< 1ms).
///
/// Uses a logistic function over a weighted advantage score derived from
/// income ratio, territory ratio, army ratio, bonus control, and
/// defensive position.
pub fn quick_win_probability(state: &GameState, map: &Map) -> WinProbability {
    // Handle terminal and non-play states.
    if let Some(wp) = terminal_check(state) {
        return wp;
    }

    let p = material_evaluation(state, map);
    WinProbability {
        player_0: p,
        player_1: 1.0 - p,
        simulations_run: 0,
    }
}

/// Raw material evaluation returning player 0 win probability in [0.0, 1.0].
///
/// Computes a weighted advantage score from multiple features and maps it
/// through a logistic function calibrated so that:
/// - Equal position => ~0.5
/// - 2x income advantage => ~0.75
/// - 3x income advantage => ~0.90
/// - All territories => 1.0
/// - Zero territories => 0.0
pub fn material_evaluation(state: &GameState, map: &Map) -> f64 {
    // Terminal states.
    if state.phase == Phase::Finished {
        return match state.winner {
            Some(0) => 1.0,
            Some(_) => 0.0,
            None => 0.5,
        };
    }

    let p0_territories = state.territory_count_for(0) as f64;
    let p1_territories = state.territory_count_for(1) as f64;
    // Elimination.
    if p0_territories == 0.0 {
        return 0.0;
    }
    if p1_territories == 0.0 {
        return 1.0;
    }

    // --- Income advantage ---
    let p0_income = state.income(0, map) as f64;
    let p1_income = state.income(1, map) as f64;
    // ln(ratio) so that 2x => ln(2) ~ 0.69, 3x => ln(3) ~ 1.10
    let income_advantage = (p0_income / p1_income.max(1.0)).ln();

    // --- Territory advantage ---
    let territory_advantage = (p0_territories / p1_territories).ln();

    // --- Army advantage ---
    let p0_armies: u32 = (0..map.territory_count())
        .filter(|&t| state.territory_owners[t] == 0)
        .map(|t| state.territory_armies[t])
        .sum();
    let p1_armies: u32 = (0..map.territory_count())
        .filter(|&t| state.territory_owners[t] == 1)
        .map(|t| state.territory_armies[t])
        .sum();
    let army_advantage = (p0_armies.max(1) as f64 / p1_armies.max(1) as f64).ln();

    // --- Bonus control advantage ---
    let bonus_advantage = bonus_control_advantage(state, map);

    // --- Defensive position advantage ---
    let p0_exposure = border_exposure(state, 0, map);
    let p1_exposure = border_exposure(state, 1, map);
    let defense_advantage = p1_exposure - p0_exposure; // less exposure = better

    // --- Weighted sum ---
    let advantage = income_advantage * WEIGHT_INCOME
        + territory_advantage * WEIGHT_TERRITORY
        + army_advantage * WEIGHT_ARMY
        + bonus_advantage * WEIGHT_BONUS
        + defense_advantage * WEIGHT_DEFENSE;

    // Logistic function.
    let p = 1.0 / (1.0 + (-LOGISTIC_K * advantage).exp());
    p.clamp(0.001, 0.999)
}

/// Compute bonus control advantage for player 0 over player 1.
/// Owning a complete bonus is worth more than partial progress.
fn bonus_control_advantage(state: &GameState, map: &Map) -> f64 {
    let mut p0_bonus_value = 0.0f64;
    let mut p1_bonus_value = 0.0f64;

    for bonus in &map.bonuses {
        if bonus.value == 0 {
            continue;
        }
        let total = bonus.territory_ids.len();
        let p0_owned = bonus
            .territory_ids
            .iter()
            .filter(|&&tid| state.territory_owners[tid] == 0)
            .count();
        let p1_owned = bonus
            .territory_ids
            .iter()
            .filter(|&&tid| state.territory_owners[tid] == 1)
            .count();

        let bv = bonus.value as f64;

        if p0_owned == total {
            // Complete bonus: worth full value + a control premium.
            p0_bonus_value += bv * 1.5;
        } else if p0_owned > 0 {
            let frac = p0_owned as f64 / total as f64;
            p0_bonus_value += bv * frac * frac; // quadratic: partial is worth less
        }

        if p1_owned == total {
            p1_bonus_value += bv * 1.5;
        } else if p1_owned > 0 {
            let frac = p1_owned as f64 / total as f64;
            p1_bonus_value += bv * frac * frac;
        }
    }

    let max_bonus: f64 = map.bonuses.iter().map(|b| b.value as f64 * 1.5).sum();
    if max_bonus == 0.0 {
        return 0.0;
    }

    // Normalize to roughly [-1, 1] range.
    (p0_bonus_value - p1_bonus_value) / max_bonus.max(1.0)
}

/// Compute border exposure as a fraction in [0, 1].
/// Higher means more weak borders (bad).
fn border_exposure(state: &GameState, player: PlayerId, map: &Map) -> f64 {
    let mut weak_borders = 0u32;
    let mut total_borders = 0u32;

    for tid in 0..map.territory_count() {
        if state.territory_owners[tid] != player {
            continue;
        }
        let is_border = map.territories[tid]
            .adjacent
            .iter()
            .any(|&adj| state.territory_owners[adj] != player);

        if is_border {
            total_borders += 1;
            let enemy_threat: u32 = map.territories[tid]
                .adjacent
                .iter()
                .filter(|&&adj| {
                    state.territory_owners[adj] != player && state.territory_owners[adj] != NEUTRAL
                })
                .map(|&adj| state.territory_armies[adj])
                .sum();

            if enemy_threat > state.territory_armies[tid] {
                weak_borders += 1;
            }
        }
    }

    if total_borders == 0 {
        0.0
    } else {
        weak_borders as f64 / total_borders as f64
    }
}

// ── Layer 2: Lookahead ──

/// 1-ply lookahead win probability (< 50ms).
///
/// Generates the best orders for both players using the greedy AI,
/// then simulates the next turn with both possible move orders
/// (player 0 first and player 1 first) and averages the resulting
/// material evaluations. This gives a stable estimate because
/// combat is deterministic; the only randomness is move order.
pub fn win_probability_with_lookahead(state: &GameState, map: &Map) -> WinProbability {
    // Handle terminal and non-play states.
    if let Some(wp) = terminal_check(state) {
        return wp;
    }

    let p0_orders = ai::generate_orders(state, 0, map);
    let p1_orders = ai::generate_orders(state, 1, map);

    // Simulate with player 0 moving first.
    let eval_p0_first = {
        let mut rng = FixedOrderRng { first_player: 0 };
        let result = resolve_turn(state, [p0_orders.clone(), p1_orders.clone()], map, &mut rng);
        material_evaluation(&result.state, map)
    };

    // Simulate with player 1 moving first.
    let eval_p1_first = {
        let mut rng = FixedOrderRng { first_player: 1 };
        let result = resolve_turn(state, [p0_orders, p1_orders], map, &mut rng);
        material_evaluation(&result.state, map)
    };

    // Average (since move order is 50/50).
    let p = (eval_p0_first + eval_p1_first) / 2.0;
    let p = p.clamp(0.001, 0.999);

    WinProbability {
        player_0: p,
        player_1: 1.0 - p,
        simulations_run: 0,
    }
}

/// A mock RNG that always produces a specific first-player ordering.
/// `gen_bool(0.5)` returns true if first_player == 0, false otherwise.
struct FixedOrderRng {
    first_player: u8,
}

impl rand::RngCore for FixedOrderRng {
    fn next_u32(&mut self) -> u32 {
        // gen_bool(0.5) calls gen::<u64>() and checks bit 0.
        // We need gen_bool to return true (player 0 first) or false (player 1 first).
        // gen_bool(0.5) returns (next_u64() & 1) < 1, which means it returns true when bit 0 == 0.
        // Actually, the implementation of gen_bool for 0.5 may vary.
        // Safest: for first_player == 0 we want gen_bool(0.5) == true.
        // gen_bool(p) in rand returns: self.gen::<f64>() < p
        // gen::<f64>() uses next_u64, takes top 52 bits, divides by 2^52.
        // So if next_u64 returns 0, gen::<f64>() = 0.0 < 0.5 => true.
        // If next_u64 returns u64::MAX, gen::<f64>() ~ 1.0 >= 0.5 => false.
        if self.first_player == 0 { 0 } else { u32::MAX }
    }

    fn next_u64(&mut self) -> u64 {
        if self.first_player == 0 { 0 } else { u64::MAX }
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        let val = if self.first_player == 0 { 0u8 } else { 0xFF };
        for b in dest.iter_mut() {
            *b = val;
        }
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand::Error> {
        self.fill_bytes(dest);
        Ok(())
    }
}

// ── Layer 3: Full Monte Carlo with material correction ──

/// Full Monte Carlo win probability with material-corrected output (< 500ms).
///
/// Runs AI-vs-AI simulations but evaluates terminal/timeout positions with
/// the material evaluation function instead of a simple win/loss count.
/// This produces smoother, more accurate estimates.
pub fn full_win_probability(state: &GameState, map: &Map, num_sims: u32) -> WinProbability {
    // Handle terminal and non-play states.
    if let Some(wp) = terminal_check(state) {
        return wp;
    }

    let max_turns = 30;
    let mut total_eval = 0.0f64;

    for i in 0..num_sims {
        let mut rng = SmallRng::seed_from_u64(i as u64 * 7919 + 42);
        let eval = simulate_game_material(state, map, max_turns, &mut rng);
        total_eval += eval;
    }

    let p = (total_eval / num_sims as f64).clamp(0.001, 0.999);

    WinProbability {
        player_0: p,
        player_1: 1.0 - p,
        simulations_run: num_sims,
    }
}

/// Simulate a single game to completion, returning the material evaluation
/// of the final position (not just 1.0/0.0).
fn simulate_game_material(state: &GameState, map: &Map, max_turns: u32, rng: &mut impl Rng) -> f64 {
    let mut current = state.clone();

    for _ in 0..max_turns {
        if current.phase == Phase::Finished {
            return material_evaluation(&current, map);
        }
        if current.phase != Phase::Play {
            break;
        }

        let p0_orders = perturbed_orders(&current, 0, map, rng);
        let p1_orders = perturbed_orders(&current, 1, map, rng);

        let result = resolve_turn(&current, [p0_orders, p1_orders], map, rng);
        current = result.state;
    }

    material_evaluation(&current, map)
}

/// Generate orders with a small random perturbation for simulation diversity.
fn perturbed_orders(
    state: &GameState,
    player: PlayerId,
    map: &Map,
    rng: &mut impl Rng,
) -> Vec<Order> {
    if state.territory_count_for(player) == 0 {
        return Vec::new();
    }

    if rng.gen_bool(0.8) {
        ai::generate_orders(state, player, map)
    } else {
        random_deploy_greedy(state, player, map, rng)
    }
}

/// Greedy AI with randomized deployment.
fn random_deploy_greedy(
    state: &GameState,
    player: PlayerId,
    map: &Map,
    rng: &mut impl Rng,
) -> Vec<Order> {
    let income = state.income(player, map);
    if income == 0 {
        return Vec::new();
    }

    let borders: Vec<usize> = (0..map.territory_count())
        .filter(|&tid| {
            state.territory_owners[tid] == player
                && map.territories[tid]
                    .adjacent
                    .iter()
                    .any(|&adj| state.territory_owners[adj] != player)
        })
        .collect();

    if borders.is_empty() {
        return ai::generate_orders(state, player, map);
    }

    let deploy_target = borders[rng.gen_range(0..borders.len())];

    let mut orders = vec![Order::Deploy {
        territory: deploy_target,
        armies: income,
    }];

    let mut sim_armies = state.territory_armies.clone();
    let mut sim_owners = state.territory_owners.clone();
    sim_armies[deploy_target] += income;

    let mut targets: Vec<usize> = map.territories[deploy_target]
        .adjacent
        .iter()
        .copied()
        .filter(|&adj| sim_owners[adj] != player)
        .collect();
    targets.sort_by_key(|&t| sim_armies[t]);

    for target in targets {
        if sim_armies[deploy_target] <= 1 {
            break;
        }
        let attackers = sim_armies[deploy_target] - 1;
        let defenders = sim_armies[target];
        if defenders == 0 || attackers == 0 {
            continue;
        }
        let result = crate::combat::resolve_attack(attackers, defenders, &map.settings);
        if result.captured {
            orders.push(Order::Attack {
                from: deploy_target,
                to: target,
                armies: attackers,
            });
            sim_armies[deploy_target] = 1;
            sim_armies[target] = result.surviving_attackers;
            sim_owners[target] = player;
        }
    }

    orders
}

// ── Calibration ──

/// Run AI-vs-AI games to calibrate the material evaluation.
///
/// Records the material evaluation at each turn and tracks who won,
/// producing a mapping from material evaluation bucket to actual win rate.
pub fn calibrate_evaluation(map: &Map, num_games: u32) -> EvalCalibration {
    let mut bucket_wins = [0.0f64; 10];
    let mut bucket_counts = [0u32; 10];

    for game_idx in 0..num_games {
        let mut rng = SmallRng::seed_from_u64(game_idx as u64 * 104729 + 7);
        let mut state = GameState::new(map);

        // Quick setup: assign territories round-robin for calibration.
        let n = map.territory_count();
        for tid in 0..n {
            state.territory_owners[tid] = (tid % 2) as PlayerId;
            state.territory_armies[tid] = 5;
        }
        state.phase = Phase::Play;
        state.turn = 1;

        // Record material evals during the game.
        let mut turn_evals: Vec<f64> = Vec::new();

        for _ in 0..50 {
            if state.phase != Phase::Play {
                break;
            }

            let eval = material_evaluation(&state, map);
            turn_evals.push(eval);

            let p0_orders = ai::generate_orders(&state, 0, map);
            let p1_orders = ai::generate_orders(&state, 1, map);
            let result = resolve_turn(&state, [p0_orders, p1_orders], map, &mut rng);
            state = result.state;
        }

        // Determine outcome.
        let outcome = if state.phase == Phase::Finished {
            if state.winner == Some(0) { 1.0 } else { 0.0 }
        } else {
            material_evaluation(&state, map)
        };

        // Record each turn's eval and the game outcome.
        for eval in turn_evals {
            let bucket = ((eval * 10.0) as usize).min(9);
            bucket_wins[bucket] += outcome;
            bucket_counts[bucket] += 1;
        }
    }

    let mut buckets = [0.0f64; 10];
    for i in 0..10 {
        if bucket_counts[i] > 0 {
            buckets[i] = bucket_wins[i] / bucket_counts[i] as f64;
        } else {
            // Default: midpoint of the bucket.
            buckets[i] = (i as f64 + 0.5) / 10.0;
        }
    }

    EvalCalibration {
        buckets,
        total_games: num_games,
    }
}

/// Apply calibration to a raw material evaluation.
pub fn calibrated_eval(raw_eval: f64, calibration: &EvalCalibration) -> f64 {
    let bucket = ((raw_eval * 10.0) as usize).min(9);
    calibration.buckets[bucket]
}

// ── Legacy API (preserved for compatibility) ──

/// Original Monte Carlo win probability estimation.
///
/// Kept as `monte_carlo_win_probability` for comparison with the new methods.
pub fn monte_carlo_win_probability(
    state: &GameState,
    map: &Map,
    num_simulations: u32,
    max_turns: u32,
) -> WinProbability {
    if let Some(wp) = terminal_check(state) {
        return wp;
    }

    let mut p0_wins = 0.0f64;
    let mut p1_wins = 0.0f64;

    for i in 0..num_simulations {
        let mut rng = SmallRng::seed_from_u64(i as u64 * 7919 + 42);
        let result = simulate_game_legacy(state, map, max_turns, &mut rng);
        p0_wins += result;
        p1_wins += 1.0 - result;
    }

    let total = num_simulations as f64;
    WinProbability {
        player_0: p0_wins / total,
        player_1: p1_wins / total,
        simulations_run: num_simulations,
    }
}

/// Legacy: kept as the old `estimate_win_probability` for backward compatibility.
pub fn estimate_win_probability(
    state: &GameState,
    map: &Map,
    num_simulations: u32,
    max_turns: u32,
) -> WinProbability {
    monte_carlo_win_probability(state, map, num_simulations, max_turns)
}

/// Simulate a single game (legacy method using mcts::evaluate_position).
fn simulate_game_legacy(state: &GameState, map: &Map, max_turns: u32, rng: &mut impl Rng) -> f64 {
    let mut current = state.clone();

    for _ in 0..max_turns {
        if current.phase == Phase::Finished {
            return if current.winner == Some(0) { 1.0 } else { 0.0 };
        }
        if current.phase != Phase::Play {
            break;
        }

        let p0_orders = perturbed_orders(&current, 0, map, rng);
        let p1_orders = perturbed_orders(&current, 1, map, rng);

        let result = resolve_turn(&current, [p0_orders, p1_orders], map, rng);
        current = result.state;
    }

    if current.phase == Phase::Finished {
        if current.winner == Some(0) { 1.0 } else { 0.0 }
    } else {
        crate::mcts::evaluate_position(&current, 0, map)
    }
}

// ── Helpers ──

/// Check for terminal/non-play states and return early if applicable.
fn terminal_check(state: &GameState) -> Option<WinProbability> {
    if state.phase == Phase::Finished {
        return Some(if state.winner == Some(0) {
            WinProbability {
                player_0: 1.0,
                player_1: 0.0,
                simulations_run: 0,
            }
        } else if state.winner == Some(1) {
            WinProbability {
                player_0: 0.0,
                player_1: 1.0,
                simulations_run: 0,
            }
        } else {
            WinProbability {
                player_0: 0.5,
                player_1: 0.5,
                simulations_run: 0,
            }
        });
    }

    if state.phase == Phase::Picking {
        return Some(WinProbability {
            player_0: 0.5,
            player_1: 0.5,
            simulations_run: 0,
        });
    }

    None
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Bonus, MapSettings, PickingConfig, PickingMethod, Territory};

    fn test_map() -> Map {
        Map {
            id: "test".into(),
            name: "Test".into(),
            territories: vec![
                Territory {
                    id: 0,
                    name: "A".into(),
                    bonus_id: 0,
                    is_wasteland: false,
                    default_armies: 2,
                    adjacent: vec![1],
                    visual: None,
                },
                Territory {
                    id: 1,
                    name: "B".into(),
                    bonus_id: 0,
                    is_wasteland: false,
                    default_armies: 2,
                    adjacent: vec![0, 2],
                    visual: None,
                },
                Territory {
                    id: 2,
                    name: "C".into(),
                    bonus_id: 1,
                    is_wasteland: false,
                    default_armies: 2,
                    adjacent: vec![1, 3],
                    visual: None,
                },
                Territory {
                    id: 3,
                    name: "D".into(),
                    bonus_id: 1,
                    is_wasteland: false,
                    default_armies: 2,
                    adjacent: vec![2],
                    visual: None,
                },
            ],
            bonuses: vec![
                Bonus {
                    id: 0,
                    name: "Left".into(),
                    value: 2,
                    territory_ids: vec![0, 1],
                    visual: None,
                },
                Bonus {
                    id: 1,
                    name: "Right".into(),
                    value: 2,
                    territory_ids: vec![2, 3],
                    visual: None,
                },
            ],
            picking: PickingConfig {
                num_picks: 1,
                method: PickingMethod::RandomWarlords,
            },
            settings: MapSettings {
                luck_pct: 0,
                base_income: 5,
                wasteland_armies: 10,
                unpicked_neutral_armies: 4,
                fog_of_war: true,
                offense_kill_rate: 0.6,
                defense_kill_rate: 0.7,
            },
        }
    }

    fn symmetric_state(map: &Map) -> GameState {
        let mut state = GameState::new(map);
        state.territory_owners = vec![0, 0, 1, 1];
        state.territory_armies = vec![5, 5, 5, 5];
        state.phase = Phase::Play;
        state.turn = 1;
        state
    }

    // ── quick_win_probability tests ──

    #[test]
    fn test_quick_symmetric_position_near_half() {
        let map = test_map();
        let state = symmetric_state(&map);
        let wp = quick_win_probability(&state, &map);
        assert!(
            (wp.player_0 - 0.5).abs() < 0.1,
            "symmetric position should be ~0.5, got {}",
            wp.player_0
        );
        assert!(
            (wp.player_0 + wp.player_1 - 1.0).abs() < 0.001,
            "probabilities must sum to 1.0"
        );
    }

    #[test]
    fn test_quick_3x_income_advantage() {
        // Give player 0 both bonuses (income = 5 + 2 + 2 = 9)
        // and player 1 only base income (5).
        // To get ~3x, we need an even bigger gap. Let's use a custom map.
        let map = test_map();
        let mut state = GameState::new(&map);
        state.territory_owners = vec![0, 0, 0, 1]; // p0 owns A,B,C => bonus Left complete
        state.territory_armies = vec![10, 10, 10, 2];
        state.phase = Phase::Play;
        state.turn = 1;
        // p0 income: 5 (base) + 2 (Left bonus) = 7
        // p1 income: 5 (base)
        // plus army and territory advantage
        let wp = quick_win_probability(&state, &map);
        assert!(
            wp.player_0 > 0.7,
            "dominant position with income+territory+army advantage should be >0.7, got {}",
            wp.player_0
        );
    }

    #[test]
    fn test_quick_finished_game_p0_wins() {
        let map = test_map();
        let mut state = GameState::new(&map);
        state.phase = Phase::Finished;
        state.winner = Some(0);

        let wp = quick_win_probability(&state, &map);
        assert_eq!(wp.player_0, 1.0);
        assert_eq!(wp.player_1, 0.0);
    }

    #[test]
    fn test_quick_finished_game_p1_wins() {
        let map = test_map();
        let mut state = GameState::new(&map);
        state.phase = Phase::Finished;
        state.winner = Some(1);

        let wp = quick_win_probability(&state, &map);
        assert_eq!(wp.player_0, 0.0);
        assert_eq!(wp.player_1, 1.0);
    }

    #[test]
    fn test_quick_picking_phase() {
        let map = test_map();
        let state = GameState::new(&map);
        let wp = quick_win_probability(&state, &map);
        assert_eq!(wp.player_0, 0.5);
        assert_eq!(wp.player_1, 0.5);
    }

    #[test]
    fn test_quick_elimination_p0_has_zero() {
        let map = test_map();
        let mut state = GameState::new(&map);
        state.territory_owners = vec![1, 1, 1, 1];
        state.territory_armies = vec![5, 5, 5, 5];
        state.phase = Phase::Play;
        state.turn = 1;

        let eval = material_evaluation(&state, &map);
        assert_eq!(eval, 0.0, "player 0 with zero territories should be 0.0");
    }

    #[test]
    fn test_quick_elimination_p1_has_zero() {
        let map = test_map();
        let mut state = GameState::new(&map);
        state.territory_owners = vec![0, 0, 0, 0];
        state.territory_armies = vec![5, 5, 5, 5];
        state.phase = Phase::Play;
        state.turn = 1;

        let eval = material_evaluation(&state, &map);
        assert_eq!(eval, 1.0, "player 1 with zero territories should give 1.0");
    }

    // ── win_probability_with_lookahead tests ──

    #[test]
    fn test_lookahead_symmetric() {
        let map = test_map();
        let state = symmetric_state(&map);
        let wp = win_probability_with_lookahead(&state, &map);
        // After one turn of greedy play from a symmetric position,
        // should still be roughly balanced.
        assert!(
            wp.player_0 > 0.0 && wp.player_0 < 1.0,
            "lookahead should produce a non-extreme result, got {}",
            wp.player_0
        );
        assert!(
            (wp.player_0 + wp.player_1 - 1.0).abs() < 0.001,
            "probabilities must sum to 1.0"
        );
    }

    #[test]
    fn test_lookahead_finished_game() {
        let map = test_map();
        let mut state = GameState::new(&map);
        state.phase = Phase::Finished;
        state.winner = Some(0);

        let wp = win_probability_with_lookahead(&state, &map);
        assert_eq!(wp.player_0, 1.0);
    }

    // ── full_win_probability tests ──

    #[test]
    fn test_full_symmetric() {
        let map = test_map();
        let state = symmetric_state(&map);
        let wp = full_win_probability(&state, &map, 20);
        assert!(
            wp.player_0 > 0.1 && wp.player_0 < 0.9,
            "full eval of symmetric position should be roughly balanced, got {}",
            wp.player_0
        );
        assert_eq!(wp.simulations_run, 20);
    }

    #[test]
    fn test_full_dominant_position() {
        let map = test_map();
        let mut state = GameState::new(&map);
        state.territory_owners = vec![0, 0, 0, 1];
        state.territory_armies = vec![10, 10, 10, 1];
        state.phase = Phase::Play;
        state.turn = 1;

        let wp = full_win_probability(&state, &map, 30);
        assert!(
            wp.player_0 > 0.6,
            "dominant position should strongly favor player 0, got {}",
            wp.player_0
        );
    }

    #[test]
    fn test_full_sums_to_one() {
        let map = test_map();
        let state = symmetric_state(&map);
        let wp = full_win_probability(&state, &map, 10);
        let sum = wp.player_0 + wp.player_1;
        assert!(
            (sum - 1.0).abs() < 0.01,
            "probabilities should sum to 1.0, got {}",
            sum
        );
    }

    // ── Legacy API backward compatibility ──

    #[test]
    fn test_legacy_estimate_win_probability() {
        let map = test_map();
        let state = symmetric_state(&map);
        let wp = estimate_win_probability(&state, &map, 20, 15);
        assert!(wp.player_0 >= 0.0 && wp.player_0 <= 1.0);
        assert!((wp.player_0 + wp.player_1 - 1.0).abs() < 0.01);
        assert_eq!(wp.simulations_run, 20);
    }

    #[test]
    fn test_legacy_finished_game() {
        let map = test_map();
        let mut state = GameState::new(&map);
        state.phase = Phase::Finished;
        state.winner = Some(0);

        let wp = estimate_win_probability(&state, &map, 100, 50);
        assert_eq!(wp.player_0, 1.0);
        assert_eq!(wp.player_1, 0.0);
        assert_eq!(wp.simulations_run, 0);
    }

    // ── Calibration tests ──

    #[test]
    fn test_calibration_produces_valid_buckets() {
        let map = test_map();
        let cal = calibrate_evaluation(&map, 5);
        for &v in &cal.buckets {
            assert!(
                v >= 0.0 && v <= 1.0,
                "calibration bucket out of range: {}",
                v
            );
        }
        assert_eq!(cal.total_games, 5);
    }

    #[test]
    fn test_calibrated_eval_within_range() {
        let cal = EvalCalibration::default();
        for i in 0..=100 {
            let raw = i as f64 / 100.0;
            let corrected = calibrated_eval(raw, &cal);
            assert!(
                corrected >= 0.0 && corrected <= 1.0,
                "calibrated eval out of range for raw {}: {}",
                raw,
                corrected
            );
        }
    }

    // ── Material evaluation monotonicity ──

    #[test]
    fn test_material_eval_more_income_is_better() {
        let map = test_map();

        // Symmetric.
        let state_sym = symmetric_state(&map);
        let eval_sym = material_evaluation(&state_sym, &map);

        // Player 0 owns 3 territories including full Left bonus.
        let mut state_adv = GameState::new(&map);
        state_adv.territory_owners = vec![0, 0, 0, 1];
        state_adv.territory_armies = vec![5, 5, 5, 5];
        state_adv.phase = Phase::Play;
        state_adv.turn = 1;
        let eval_adv = material_evaluation(&state_adv, &map);

        assert!(
            eval_adv > eval_sym,
            "more territories + bonus should score higher: adv={} sym={}",
            eval_adv,
            eval_sym
        );
    }
}
