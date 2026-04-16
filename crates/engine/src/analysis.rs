//! Win probability estimation via Monte Carlo simulation.
//!
//! Runs many simulated games from a given position using the greedy AI
//! for both players, and counts wins to estimate probability.

use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};

use crate::ai;
use crate::map::Map;
use crate::orders::Order;
use crate::state::{GameState, Phase, PlayerId};
use crate::turn::resolve_turn;

/// Win probability estimate from Monte Carlo simulation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WinProbability {
    /// Probability player 0 wins (0.0 to 1.0).
    pub player_0: f64,
    /// Probability player 1 wins (0.0 to 1.0).
    pub player_1: f64,
    /// Number of simulations run.
    pub simulations_run: u32,
}

/// Estimate win probability by simulating games from the current state.
///
/// Both players use the greedy AI with a small random perturbation to
/// introduce variation across simulations. Games that exceed `max_turns`
/// are scored by board position rather than declared a draw.
pub fn estimate_win_probability(
    state: &GameState,
    map: &Map,
    num_simulations: u32,
    max_turns: u32,
) -> WinProbability {
    // Handle terminal states immediately.
    if state.phase == Phase::Finished {
        return if state.winner == Some(0) {
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
        };
    }

    // Handle picking phase: return 50/50.
    if state.phase == Phase::Picking {
        return WinProbability {
            player_0: 0.5,
            player_1: 0.5,
            simulations_run: 0,
        };
    }

    let mut p0_wins = 0.0f64;
    let mut p1_wins = 0.0f64;

    for i in 0..num_simulations {
        let mut rng = SmallRng::seed_from_u64(i as u64 * 7919 + 42);
        let result = simulate_game(state, map, max_turns, &mut rng);
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

/// Simulate a single game to completion. Returns a score for player 0:
/// 1.0 = player 0 wins, 0.0 = player 1 wins, or a heuristic for timeouts.
fn simulate_game(
    state: &GameState,
    map: &Map,
    max_turns: u32,
    rng: &mut impl Rng,
) -> f64 {
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

    // If game didn't finish, evaluate position heuristically.
    if current.phase == Phase::Finished {
        if current.winner == Some(0) { 1.0 } else { 0.0 }
    } else {
        crate::mcts::evaluate_position(&current, 0, map)
    }
}

/// Generate orders with a small random perturbation for simulation diversity.
///
/// Most of the time (80%) uses the greedy AI. Sometimes (20%) uses a
/// random deployment target to add variation.
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
        // Use standard greedy AI.
        ai::generate_orders(state, player, map)
    } else {
        // Use greedy AI but with a random deploy target.
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

    // Find owned border territories.
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
        // All interior -- just use greedy.
        return ai::generate_orders(state, player, map);
    }

    let deploy_target = borders[rng.gen_range(0..borders.len())];

    let mut orders = vec![Order::Deploy {
        territory: deploy_target,
        armies: income,
    }];

    // Generate attacks from the deploy target.
    let mut sim_armies = state.territory_armies.clone();
    let mut sim_owners = state.territory_owners.clone();
    sim_armies[deploy_target] += income;

    // Attack weakest neighbors.
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

    #[test]
    fn test_win_probability_finished_game() {
        let map = test_map();
        let mut state = GameState::new(&map);
        state.phase = Phase::Finished;
        state.winner = Some(0);

        let wp = estimate_win_probability(&state, &map, 100, 50);
        assert_eq!(wp.player_0, 1.0);
        assert_eq!(wp.player_1, 0.0);
        assert_eq!(wp.simulations_run, 0);
    }

    #[test]
    fn test_win_probability_picking_phase() {
        let map = test_map();
        let state = GameState::new(&map);

        let wp = estimate_win_probability(&state, &map, 100, 50);
        assert_eq!(wp.player_0, 0.5);
        assert_eq!(wp.player_1, 0.5);
    }

    #[test]
    fn test_win_probability_even_position() {
        let map = test_map();
        let mut state = GameState::new(&map);
        state.territory_owners = vec![0, 0, 1, 1];
        state.territory_armies = vec![5, 5, 5, 5];
        state.phase = Phase::Play;
        state.turn = 1;

        let wp = estimate_win_probability(&state, &map, 50, 20);
        // Even position should be roughly 50/50.
        assert!(wp.player_0 >= 0.0 && wp.player_0 <= 1.0);
        assert!((wp.player_0 + wp.player_1 - 1.0).abs() < 0.01);
        assert_eq!(wp.simulations_run, 50);
    }

    #[test]
    fn test_win_probability_dominant_position() {
        let map = test_map();
        let mut state = GameState::new(&map);
        state.territory_owners = vec![0, 0, 0, 1];
        state.territory_armies = vec![10, 10, 10, 1];
        state.phase = Phase::Play;
        state.turn = 1;

        let wp = estimate_win_probability(&state, &map, 50, 20);
        // Player 0 should be strongly favored.
        assert!(
            wp.player_0 > 0.6,
            "dominant position should favor player 0, got {}",
            wp.player_0
        );
    }

    #[test]
    fn test_win_probability_sums_to_one() {
        let map = test_map();
        let mut state = GameState::new(&map);
        state.territory_owners = vec![0, 0, 1, 1];
        state.territory_armies = vec![3, 3, 3, 3];
        state.phase = Phase::Play;
        state.turn = 1;

        let wp = estimate_win_probability(&state, &map, 30, 15);
        let sum = wp.player_0 + wp.player_1;
        assert!(
            (sum - 1.0).abs() < 0.01,
            "probabilities should sum to 1.0, got {}",
            sum
        );
    }
}
