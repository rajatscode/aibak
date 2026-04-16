use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::combat::resolve_attack;
use crate::map::Map;
use crate::mcts::{MctsConfig, mcts_generate_orders};
use crate::orders::Order;
use crate::state::{GameState, PlayerId, NEUTRAL};

/// AI difficulty / strategy profile.
#[derive(Debug, Clone, Copy)]
pub enum AiProfile {
    /// Greedy single-step evaluation.
    Standard,
    /// Multi-step planning with expansion focus.
    Aggressive,
}

/// AI strength level for player-facing difficulty selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AiStrength {
    /// Random deployment and attacks (weakest).
    Easy,
    /// Greedy heuristic (existing AI).
    Medium,
    /// MCTS-based search (strongest).
    Hard,
}

impl Default for AiStrength {
    fn default() -> Self {
        Self::Hard
    }
}

/// Generate orders using the specified AI strength level.
pub fn generate_orders_for_strength(
    state: &GameState,
    player: PlayerId,
    map: &Map,
    strength: AiStrength,
) -> Vec<Order> {
    match strength {
        AiStrength::Easy => generate_random_orders(state, player, map),
        AiStrength::Medium => generate_orders(state, player, map),
        AiStrength::Hard => {
            let config = MctsConfig {
                time_budget: Duration::from_millis(500),
                ..Default::default()
            };
            mcts_generate_orders(state, player, map, &config)
        }
    }
}

/// Generate random (easy) orders: deploy all income on a random border territory,
/// then attack a random neighbor if possible.
fn generate_random_orders(state: &GameState, player: PlayerId, map: &Map) -> Vec<Order> {
    use rand::seq::SliceRandom;

    let income = state.income(player, map);
    if income == 0 {
        return Vec::new();
    }

    let owned: Vec<usize> = (0..map.territory_count())
        .filter(|&tid| state.territory_owners[tid] == player)
        .collect();

    if owned.is_empty() {
        return Vec::new();
    }

    let mut rng = rand::thread_rng();
    let deploy_target = *owned.choose(&mut rng).unwrap();

    let mut orders = vec![Order::Deploy {
        territory: deploy_target,
        armies: income,
    }];

    // Maybe attack a random neighbor.
    let mut sim_armies = state.territory_armies.clone();
    sim_armies[deploy_target] += income;

    let attackable: Vec<usize> = map.territories[deploy_target]
        .adjacent
        .iter()
        .copied()
        .filter(|&adj| state.territory_owners[adj] != player)
        .collect();

    if let Some(&target) = attackable.choose(&mut rng) {
        if sim_armies[deploy_target] > 1 {
            orders.push(Order::Attack {
                from: deploy_target,
                to: target,
                armies: sim_armies[deploy_target] - 1,
            });
        }
    }

    orders
}

/// Generate AI orders with multi-step attack planning.
pub fn generate_orders(state: &GameState, player: PlayerId, map: &Map) -> Vec<Order> {
    generate_orders_with_profile(state, player, map, AiProfile::Aggressive)
}

pub fn generate_orders_with_profile(
    state: &GameState,
    player: PlayerId,
    map: &Map,
    _profile: AiProfile,
) -> Vec<Order> {
    let income = state.income(player, map);
    if income == 0 {
        return Vec::new();
    }

    // ========== CLEANUP MODE ==========
    // When the AI owns >60% of the map, switch to aggressive cleanup strategy
    // that spreads deployment and attacks from every border.
    let total_territories = map.territory_count();
    let ai_territories = state.territory_count_for(player);
    if ai_territories as f64 / total_territories as f64 > 0.6 {
        return generate_cleanup_orders(state, player, map);
    }

    let mut orders = Vec::new();
    let opp = 1 - player;

    // ========== ENDGAME DETECTION ==========
    let my_territory_count = state.territory_count_for(player);
    let opp_territory_count = state.territory_count_for(opp);
    let total_territories = map.territory_count();
    let my_share = my_territory_count as f64 / total_territories as f64;
    let territory_ratio = if opp_territory_count > 0 {
        my_territory_count as f64 / opp_territory_count as f64
    } else {
        100.0
    };
    let my_income = state.income(player, map);
    let opp_income = state.income(opp, map);
    let income_ratio = if opp_income > 0 {
        my_income as f64 / opp_income as f64
    } else {
        100.0
    };
    let endgame = my_share > 0.60;
    let dominant = territory_ratio >= 2.0 || income_ratio >= 3.0;

    // ========== ANALYZE BONUSES ==========
    // Score each bonus by completion proximity and strategic value
    let mut bonus_priorities: Vec<BonusTarget> = Vec::new();
    for bonus in &map.bonuses {
        if bonus.value == 0 {
            continue;
        }
        let owned: Vec<usize> = bonus
            .territory_ids
            .iter()
            .copied()
            .filter(|&tid| state.territory_owners[tid] == player)
            .collect();
        let missing: Vec<usize> = bonus
            .territory_ids
            .iter()
            .copied()
            .filter(|&tid| state.territory_owners[tid] != player)
            .collect();

        if missing.is_empty() {
            continue; // Already own this bonus
        }

        // Reachable missing: territories adjacent to something we own
        let reachable: Vec<usize> = missing
            .iter()
            .copied()
            .filter(|&tid| {
                map.territories[tid]
                    .adjacent
                    .iter()
                    .any(|&adj| state.territory_owners[adj] == player)
            })
            .collect();

        if owned.is_empty() || reachable.is_empty() {
            continue;
        }

        // Cost to take reachable territories
        let cost: u32 = reachable.iter().map(|&tid| armies_to_take(state.territory_armies[tid], &map.settings)).sum();

        let completion = owned.len() as f64 / bonus.territory_ids.len() as f64;
        let efficiency = bonus.value as f64 / bonus.territory_ids.len() as f64;
        let affordable = if cost == 0 { 10.0 } else { (income as f64 + 5.0) / cost as f64 };

        // Penalize bonuses the opponent is also contesting
        let opp_owned = bonus
            .territory_ids
            .iter()
            .filter(|&&tid| state.territory_owners[tid] == opp)
            .count();
        let contest_penalty = if opp_owned > 0 { 0.5 } else { 1.0 };

        let score = (completion * 4.0 + efficiency * 2.0 + affordable.min(3.0)) * contest_penalty;

        bonus_priorities.push(BonusTarget {
            bonus_id: bonus.id,
            score,
            reachable_missing: reachable,
            cost,
        });
    }
    bonus_priorities.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());

    // ========== PLAN ATTACKS ==========
    // Build attack chain: try to capture multiple territories in sequence
    let mut attack_plan: Vec<PlannedAttack> = Vec::new();
    let mut sim_armies = state.territory_armies.clone();
    let mut sim_owners = state.territory_owners.clone();

    // First pass: plan attacks toward highest-priority bonus
    for bt in &bonus_priorities {
        for &target in &bt.reachable_missing {
            if sim_owners[target] == player {
                continue; // Already captured in simulation
            }
            // Find the best source (owned, adjacent, most armies)
            let source = map.territories[target]
                .adjacent
                .iter()
                .copied()
                .filter(|&adj| sim_owners[adj] == player)
                .max_by_key(|&adj| sim_armies[adj]);

            if let Some(src) = source {
                if sim_armies[src] <= 1 {
                    continue;
                }
                let attackers = sim_armies[src] - 1;
                let defenders = sim_armies[target];
                if defenders == 0 || attackers == 0 {
                    continue;
                }
                let result = resolve_attack(attackers, defenders, &map.settings);

                if result.captured {
                    attack_plan.push(PlannedAttack {
                        from: src,
                        to: target,
                        armies: attackers,
                        priority: bt.score,
                    });
                    sim_armies[src] = 1;
                    sim_armies[target] = result.surviving_attackers;
                    sim_owners[target] = player;
                }
            }
        }
    }

    // Second pass: look for any other easy captures (2-army neutrals)
    for tid in 0..map.territory_count() {
        if sim_owners[tid] == player {
            continue;
        }
        if sim_armies[tid] > 3 || sim_armies[tid] == 0 {
            continue; // Not easy or empty
        }
        let source = map.territories[tid]
            .adjacent
            .iter()
            .copied()
            .filter(|&adj| sim_owners[adj] == player && sim_armies[adj] > 1)
            .max_by_key(|&adj| sim_armies[adj]);

        if let Some(src) = source {
            let attackers = sim_armies[src] - 1;
            let defenders = sim_armies[tid];
            if defenders == 0 || attackers == 0 {
                continue;
            }
            let result = resolve_attack(attackers, defenders, &map.settings);

            if result.captured && !attack_plan.iter().any(|a| a.from == src) {
                attack_plan.push(PlannedAttack {
                    from: src,
                    to: tid,
                    armies: attackers,
                    priority: 1.0,
                });
                sim_armies[src] = 1;
                sim_armies[tid] = result.surviving_attackers;
                sim_owners[tid] = player;
            }
        }
    }

    // ========== DECIDE DEPLOYMENT ==========
    // In endgame mode, spread deployment across border territories.
    // Otherwise deploy to the best attack source or most threatened border.
    let deploy_target = if endgame {
        // Deploy to the border territory that can reach the most enemy territories.
        find_best_endgame_deploy(state, player, map)
            .unwrap_or_else(|| {
                find_most_threatened_border(state, player, map)
                    .unwrap_or(0)
            })
    } else if let Some(best) = attack_plan.first() {
        best.from
    } else {
        // No attacks — deploy defensively on most threatened border
        find_most_threatened_border(state, player, map)
            .unwrap_or_else(|| {
                // Fallback: first owned territory
                (0..map.territory_count())
                    .find(|&tid| state.territory_owners[tid] == player)
                    .unwrap_or(0)
            })
    };

    orders.push(Order::Deploy {
        territory: deploy_target,
        armies: income,
    });

    // ========== RE-SIMULATE WITH DEPLOYMENT ==========
    // Reset simulation with deployment included
    let mut sim_armies = state.territory_armies.clone();
    let mut sim_owners = state.territory_owners.clone();
    sim_armies[deploy_target] += income;

    let mut used_sources = vec![false; map.territory_count()];
    // Cap total attack orders to prevent unreasonably long chains (Bug 4).
    let max_attacks = map.territory_count().min(20);
    let mut attack_count = 0usize;

    // Re-evaluate attacks with new army counts
    attack_plan.clear();

    // Rebuild attack list with deployed armies
    for bt in &bonus_priorities {
        if attack_count >= max_attacks {
            break;
        }
        for &target in &bt.reachable_missing {
            if attack_count >= max_attacks {
                break;
            }
            if sim_owners[target] == player {
                continue;
            }
            let source = map.territories[target]
                .adjacent
                .iter()
                .copied()
                .filter(|&adj| sim_owners[adj] == player && !used_sources[adj])
                .max_by_key(|&adj| sim_armies[adj]);

            if let Some(src) = source {
                if sim_armies[src] <= 1 {
                    continue;
                }
                let attackers = sim_armies[src] - 1;
                let defenders = sim_armies[target];
                if defenders == 0 || attackers == 0 {
                    continue;
                }
                let result = resolve_attack(attackers, defenders, &map.settings);

                if result.captured {
                    orders.push(Order::Attack {
                        from: src,
                        to: target,
                        armies: attackers,
                    });
                    sim_armies[src] = 1;
                    sim_armies[target] = result.surviving_attackers;
                    sim_owners[target] = player;
                    used_sources[src] = true;
                    attack_count += 1;
                }
            }
        }
    }

    // Opportunistic attacks on weak neighbors (raise threshold when dominant)
    let weak_threshold = if dominant || endgame { u32::MAX } else { 3 };
    for tid in 0..map.territory_count() {
        if attack_count >= max_attacks {
            break;
        }
        if sim_owners[tid] == player || sim_armies[tid] == 0 {
            continue;
        }
        // In endgame/dominant mode, attack any enemy territory; otherwise only weak ones.
        if !dominant && !endgame && sim_armies[tid] > weak_threshold {
            continue;
        }
        let source = map.territories[tid]
            .adjacent
            .iter()
            .copied()
            .filter(|&adj| sim_owners[adj] == player && !used_sources[adj] && sim_armies[adj] > 1)
            .max_by_key(|&adj| sim_armies[adj]);

        if let Some(src) = source {
            let attackers = sim_armies[src] - 1;
            let defenders = sim_armies[tid];
            if defenders == 0 || attackers == 0 {
                continue;
            }
            let result = resolve_attack(attackers, defenders, &map.settings);

            if result.captured {
                orders.push(Order::Attack {
                    from: src,
                    to: tid,
                    armies: attackers,
                });
                sim_armies[src] = 1;
                sim_armies[tid] = result.surviving_attackers;
                sim_owners[tid] = player;
                used_sources[src] = true;
                attack_count += 1;
            }
        }
    }

    // ========== ENDGAME: ATTACK FROM EVERY BORDER ==========
    // When in endgame mode (>70% territories), attack from all border territories
    // that haven't been used yet, even if the attack isn't guaranteed to capture.
    if endgame {
        for tid in 0..map.territory_count() {
            if attack_count >= max_attacks {
                break;
            }
            if sim_owners[tid] != player || used_sources[tid] || sim_armies[tid] <= 1 {
                continue;
            }
            // Find an enemy neighbor to attack
            let enemy_neighbor = map.territories[tid]
                .adjacent
                .iter()
                .copied()
                .filter(|&adj| sim_owners[adj] != player)
                .min_by_key(|&adj| sim_armies[adj]);

            if let Some(target) = enemy_neighbor {
                let attackers = sim_armies[tid] - 1;
                let defenders = sim_armies[target];
                if attackers == 0 || defenders == 0 {
                    continue;
                }
                let result = resolve_attack(attackers, defenders, &map.settings);
                if result.captured {
                    orders.push(Order::Attack {
                        from: tid,
                        to: target,
                        armies: attackers,
                    });
                    sim_armies[tid] = 1;
                    sim_armies[target] = result.surviving_attackers;
                    sim_owners[target] = player;
                    used_sources[tid] = true;
                    attack_count += 1;
                }
            }
        }
    }

    // ========== TRANSFERS ==========
    // Move interior armies toward the front
    for tid in 0..map.territory_count() {
        if sim_owners[tid] != player || used_sources[tid] || sim_armies[tid] <= 1 {
            continue;
        }
        let is_interior = map.territories[tid]
            .adjacent
            .iter()
            .all(|&adj| sim_owners[adj] == player);
        if !is_interior {
            continue;
        }
        // Transfer toward the neighbor that borders the most enemies
        if let Some(&toward) = map.territories[tid]
            .adjacent
            .iter()
            .max_by_key(|&&adj| {
                map.territories[adj]
                    .adjacent
                    .iter()
                    .filter(|&&a2| sim_owners[a2] != player)
                    .count()
            })
        {
            let amount = sim_armies[tid] - 1;
            if amount > 0 {
                orders.push(Order::Transfer {
                    from: tid,
                    to: toward,
                    armies: amount,
                });
                sim_armies[tid] = 1;
                sim_armies[toward] += amount;
            }
        }
    }

    orders
}

/// Cleanup mode: spread deployment across all border territories, attack every
/// capturable target, and transfer interior armies toward the front.
fn generate_cleanup_orders(state: &GameState, player: PlayerId, map: &Map) -> Vec<Order> {
    let mut orders = Vec::new();
    let income = state.income(player, map);
    if income == 0 {
        return orders;
    }

    // Find all border territories (owned territories adjacent to at least one enemy)
    let mut border_territories: Vec<usize> = Vec::new();
    for tid in 0..map.territory_count() {
        if state.territory_owners[tid] != player {
            continue;
        }
        let has_enemy_neighbor = map.territories[tid]
            .adjacent
            .iter()
            .any(|&adj| state.territory_owners[adj] != player);
        if has_enemy_neighbor {
            border_territories.push(tid);
        }
    }

    if border_territories.is_empty() {
        return orders;
    }

    // 1. Spread income evenly across all border territories
    let per_territory = income / border_territories.len() as u32;
    let remainder = income % border_territories.len() as u32;
    for (i, &tid) in border_territories.iter().enumerate() {
        let amount = per_territory + if (i as u32) < remainder { 1 } else { 0 };
        if amount > 0 {
            orders.push(Order::Deploy {
                territory: tid,
                armies: amount,
            });
        }
    }

    // Build simulated army state with deployments applied
    let mut sim_armies = state.territory_armies.clone();
    let mut sim_owners = state.territory_owners.clone();
    for order in &orders {
        if let Order::Deploy { territory, armies } = order {
            sim_armies[*territory] += armies;
        }
    }

    // 2. Attack every adjacent non-owned territory where capture is possible
    let max_attacks = map.territory_count().min(30);
    let mut attack_count = 0usize;
    let mut used_sources = vec![false; map.territory_count()];

    for &tid in &border_territories {
        if attack_count >= max_attacks {
            break;
        }
        if used_sources[tid] || sim_armies[tid] <= 1 {
            continue;
        }
        // Attack the weakest adjacent enemy
        let targets: Vec<usize> = map.territories[tid]
            .adjacent
            .iter()
            .copied()
            .filter(|&adj| sim_owners[adj] != player)
            .collect();

        for target in targets {
            if attack_count >= max_attacks {
                break;
            }
            if sim_armies[tid] <= 1 {
                break;
            }
            let attackers = sim_armies[tid] - 1;
            let defenders = sim_armies[target];
            if defenders == 0 || attackers == 0 {
                continue;
            }
            let result = resolve_attack(attackers, defenders, &map.settings);
            if result.captured {
                orders.push(Order::Attack {
                    from: tid,
                    to: target,
                    armies: attackers,
                });
                sim_armies[tid] = 1;
                sim_armies[target] = result.surviving_attackers;
                sim_owners[target] = player;
                used_sources[tid] = true;
                attack_count += 1;
                break; // This source is spent
            }
        }
    }

    // 3. Transfer interior armies toward the front
    for tid in 0..map.territory_count() {
        if sim_owners[tid] != player || used_sources[tid] || sim_armies[tid] <= 1 {
            continue;
        }
        let is_interior = map.territories[tid]
            .adjacent
            .iter()
            .all(|&adj| sim_owners[adj] == player);
        if !is_interior {
            continue;
        }
        // Transfer toward the neighbor closest to an enemy
        if let Some(&toward) = map.territories[tid]
            .adjacent
            .iter()
            .max_by_key(|&&adj| {
                map.territories[adj]
                    .adjacent
                    .iter()
                    .filter(|&&a2| sim_owners[a2] != player)
                    .count()
            })
        {
            let amount = sim_armies[tid] - 1;
            if amount > 0 {
                orders.push(Order::Transfer {
                    from: tid,
                    to: toward,
                    armies: amount,
                });
                sim_armies[tid] = 1;
                sim_armies[toward] += amount;
            }
        }
    }

    orders
}

/// Calculate minimum armies needed to capture a territory with given defenders.
fn armies_to_take(defenders: u32, settings: &crate::map::MapSettings) -> u32 {
    // At 0% luck, we need ceil(defenders / offense_kill_rate) + defense_kills + 1
    // Simplified: need roughly defenders * 2 to guarantee capture
    let offense_kills_needed = defenders;
    let attackers_needed = (offense_kills_needed as f64 / settings.offense_kill_rate).ceil() as u32;
    let defense_kills = (defenders as f64 * settings.defense_kill_rate).round() as u32;
    attackers_needed.max(defense_kills + 1) + 1
}

/// Find the best border territory for endgame deployment: the one adjacent to
/// the most enemy territories (maximize attack surface for finishing the game).
fn find_best_endgame_deploy(
    state: &GameState,
    player: PlayerId,
    map: &Map,
) -> Option<usize> {
    let mut best = None;
    let mut best_score = 0usize;
    for tid in 0..map.territory_count() {
        if state.territory_owners[tid] != player {
            continue;
        }
        let enemy_neighbors: usize = map.territories[tid]
            .adjacent
            .iter()
            .filter(|&&adj| state.territory_owners[adj] != player)
            .count();
        if enemy_neighbors > best_score {
            best_score = enemy_neighbors;
            best = Some(tid);
        }
    }
    best
}

/// Find the owned border territory that faces the most enemy armies.
fn find_most_threatened_border(
    state: &GameState,
    player: PlayerId,
    map: &Map,
) -> Option<usize> {
    let mut best = None;
    let mut best_threat = 0u32;
    for tid in 0..map.territory_count() {
        if state.territory_owners[tid] != player {
            continue;
        }
        let threat: u32 = map.territories[tid]
            .adjacent
            .iter()
            .filter(|&&adj| state.territory_owners[adj] != player && state.territory_owners[adj] != NEUTRAL)
            .map(|&adj| state.territory_armies[adj])
            .sum();
        if threat > best_threat || (threat == best_threat && best.is_none()) {
            best_threat = threat;
            best = Some(tid);
        }
    }
    // If no enemy threat, find border with most neutral neighbors
    if best.is_none() {
        let mut best_neutrals = 0;
        for tid in 0..map.territory_count() {
            if state.territory_owners[tid] != player {
                continue;
            }
            let neutrals: usize = map.territories[tid]
                .adjacent
                .iter()
                .filter(|&&adj| state.territory_owners[adj] != player)
                .count();
            if neutrals > best_neutrals {
                best_neutrals = neutrals;
                best = Some(tid);
            }
        }
    }
    best
}

/// Generate picks for the AI. Prefers territories in small, high-value bonuses.
pub fn generate_picks(state: &GameState, map: &Map) -> Vec<usize> {
    let mut scored: Vec<(usize, f64)> = (0..map.territory_count())
        .filter(|&tid| !map.territories[tid].is_wasteland && state.territory_owners[tid] == NEUTRAL)
        .map(|tid| {
            let bonus = &map.bonuses[map.territories[tid].bonus_id];
            let efficiency = bonus.value as f64 / bonus.territory_ids.len() as f64;
            let defensibility = 1.0 / map.territories[tid].adjacent.len() as f64;
            // Prefer picking in different bonuses for strategic spread
            (tid, efficiency * 4.0 + defensibility)
        })
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    scored.into_iter().map(|(tid, _)| tid).collect()
}

// Internal types

#[allow(dead_code)]
struct BonusTarget {
    bonus_id: usize,
    score: f64,
    reachable_missing: Vec<usize>,
    cost: u32,
}

#[allow(dead_code)]
struct PlannedAttack {
    from: usize,
    to: usize,
    armies: u32,
    priority: f64,
}
