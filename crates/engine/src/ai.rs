//! AI order generation with multiple strength levels (easy, medium, hard).
//! Includes greedy heuristic planning and MCTS-based search.

use std::collections::VecDeque;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::board::Board;
use crate::combat::resolve_attack;
use crate::mcts::{MctsConfig, mcts_generate_orders};
use crate::orders::Order;
use crate::state::{GameState, NEUTRAL, PlayerId};

/// AI difficulty / strategy profile.
#[derive(Debug, Clone, Copy)]
pub enum AiProfile {
    /// Greedy single-step evaluation.
    Standard,
    /// Multi-step planning with expansion focus.
    Aggressive,
}

/// AI strength level for player-facing difficulty selection.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AiStrength {
    /// Random deployment and attacks (weakest).
    Easy,
    /// Greedy heuristic (existing AI).
    Medium,
    /// MCTS-based search (strongest).
    #[default]
    Hard,
}

/// Generate orders using the specified AI strength level.
pub fn generate_orders_for_strength(
    state: &GameState,
    player: PlayerId,
    board: &Board,
    strength: AiStrength,
) -> Vec<Order> {
    match strength {
        AiStrength::Easy => generate_random_orders(state, player, board),
        AiStrength::Medium => generate_orders(state, player, board),
        AiStrength::Hard => {
            let config = MctsConfig {
                time_budget: Duration::from_millis(500),
                ..Default::default()
            };
            mcts_generate_orders(state, player, board, &config)
        }
    }
}

/// Generate random (easy) orders: deploy all income on a random border territory,
/// then attack a random neighbor if possible.
fn generate_random_orders(state: &GameState, player: PlayerId, board: &Board) -> Vec<Order> {
    use rand::seq::SliceRandom;
    let map = &board.map;

    let income = state.income(player, board);
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

    if let Some(&target) = attackable.choose(&mut rng)
        && sim_armies[deploy_target] > 1
    {
        orders.push(Order::Attack {
            from: deploy_target,
            to: target,
            armies: sim_armies[deploy_target] - 1,
        });
    }

    orders
}

/// Generate AI orders with multi-step attack planning.
pub fn generate_orders(state: &GameState, player: PlayerId, board: &Board) -> Vec<Order> {
    generate_orders_with_profile(state, player, board, AiProfile::Aggressive)
}

/// Generate AI orders using a specific strategy profile.
pub fn generate_orders_with_profile(
    state: &GameState,
    player: PlayerId,
    board: &Board,
    _profile: AiProfile,
) -> Vec<Order> {
    let map = &board.map;
    let income = state.income(player, board);
    if income == 0 {
        return Vec::new();
    }

    // ========== CLEANUP MODE ==========
    // When the AI owns >60% of the map, switch to aggressive cleanup strategy
    // that spreads deployment and attacks from every border.
    let total_territories = map.territory_count();
    let ai_territories = state.territory_count_for(player);
    if ai_territories as f64 / total_territories as f64 > 0.6 {
        return generate_cleanup_orders(state, player, board);
    }

    let mut orders = Vec::new();
    let opp = 1 - player;

    // ========== SITUATION ASSESSMENT ==========
    let my_territory_count = state.territory_count_for(player);
    let opp_territory_count = state.territory_count_for(opp);
    let total_territories = map.territory_count();
    let my_share = my_territory_count as f64 / total_territories as f64;
    let territory_ratio = if opp_territory_count > 0 {
        my_territory_count as f64 / opp_territory_count as f64
    } else {
        100.0
    };
    let my_income = state.income(player, board);
    let opp_income = state.income(opp, board);
    let income_ratio = if opp_income > 0 {
        my_income as f64 / opp_income as f64
    } else {
        100.0
    };
    let endgame = my_share > 0.60;
    let dominant = territory_ratio >= 2.0 || income_ratio >= 3.0;
    let behind = territory_ratio < 0.7 || income_ratio < 0.6;
    let is_opening = state.turn <= 3;

    // ========== ANALYZE BONUSES (own + opponent) ==========
    // Score each bonus by completion proximity and strategic value
    let mut bonus_priorities: Vec<BonusTarget> = Vec::new();
    // Track opponent near-complete bonuses for counter-expansion
    let mut counter_targets: Vec<(usize, f64)> = Vec::new(); // (territory_id, priority)

    for bonus in &map.bonuses {
        if bonus.value == 0 {
            continue;
        }

        // --- Counter-expansion: detect opponent near-complete bonuses ---
        let opp_owned_in_bonus: Vec<usize> = bonus
            .territory_ids
            .iter()
            .copied()
            .filter(|&tid| state.territory_owners[tid] == opp)
            .collect();
        let opp_missing: Vec<usize> = bonus
            .territory_ids
            .iter()
            .copied()
            .filter(|&tid| state.territory_owners[tid] != opp)
            .collect();
        // If opponent owns all-but-one (or all-but-two) in a bonus, those missing
        // territories become high-priority attack targets to deny the bonus.
        if !opp_owned_in_bonus.is_empty()
            && opp_missing.len() <= 2
            && opp_missing.len() < bonus.territory_ids.len()
        {
            for &tid in &opp_missing {
                // Only relevant if we can reach it (adjacent to something we own)
                let reachable = map.territories[tid]
                    .adjacent
                    .iter()
                    .any(|&adj| state.territory_owners[adj] == player);
                // Also relevant if we already own it (defend it)
                let we_own = state.territory_owners[tid] == player;
                if reachable || we_own {
                    // Higher priority for more valuable bonuses and closer-to-complete
                    let urgency = if opp_missing.len() == 1 { 2.0 } else { 1.0 };
                    let priority = bonus.value as f64 * urgency;
                    counter_targets.push((tid, priority));
                }
            }
        }

        // --- Own bonus completion analysis ---
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
        let cost: u32 = reachable
            .iter()
            .map(|&tid| armies_to_take(state.territory_armies[tid], board.settings()))
            .sum();

        let completion = owned.len() as f64 / bonus.territory_ids.len() as f64;
        let efficiency = bonus.value as f64 / bonus.territory_ids.len() as f64;
        let affordable = if cost == 0 {
            10.0
        } else {
            (income as f64 + 5.0) / cost as f64
        };

        // Penalize bonuses the opponent is also contesting
        let opp_owned = opp_owned_in_bonus.len();
        let contest_penalty = if opp_owned > 0 { 0.5 } else { 1.0 };

        let mut score =
            (completion * 4.0 + efficiency * 2.0 + affordable.min(3.0)) * contest_penalty;

        // Opening strategy (improvement #4): in first 3 turns, heavily boost
        // the bonus requiring fewest captures so we focus on completing it.
        if is_opening {
            let captures_needed = missing.len();
            // Fewer captures needed = much higher score
            score += 10.0 / (captures_needed as f64 + 0.5);
        }

        bonus_priorities.push(BonusTarget {
            bonus_id: bonus.id,
            score,
            reachable_missing: reachable,
            cost,
        });
    }
    bonus_priorities.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
    counter_targets.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

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
                let result = resolve_attack(attackers, defenders, board.settings());

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

    // Counter-expansion pass: attack territories that would deny opponent bonuses.
    for &(target, priority) in &counter_targets {
        if sim_owners[target] == player {
            continue; // We already own it — good, it's denied
        }
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
            let result = resolve_attack(attackers, defenders, board.settings());

            if result.captured && !attack_plan.iter().any(|a| a.to == target) {
                attack_plan.push(PlannedAttack {
                    from: src,
                    to: target,
                    armies: attackers,
                    priority: priority + 5.0, // High priority for counter-expansion
                });
                sim_armies[src] = 1;
                sim_armies[target] = result.surviving_attackers;
                sim_owners[target] = player;
            }
        }
    }

    // Second pass: look for any other easy captures (skip in opening — stay focused)
    if !is_opening {
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
                let result = resolve_attack(attackers, defenders, board.settings());

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
    }

    // ========== DECIDE DEPLOYMENT (Stack vs Spread — improvement #2) ==========
    // Decision factors:
    // - Behind: stack on one front for a breakthrough
    // - Ahead/dominant: spread to defend completed bonuses
    // - Even: stack toward the most valuable incomplete bonus
    // - Opening: always stack toward nearest bonus completion
    // - Counter-expansion: if there's a high-priority counter target, consider deploying there

    let deploy_target = if endgame {
        // Deploy to the border territory that can reach the most enemy territories.
        find_best_endgame_deploy(state, player, board)
            .unwrap_or_else(|| find_most_threatened_border(state, player, board).unwrap_or(0))
    } else if is_opening || behind {
        // Stack: concentrate all income on the single best attack source.
        // In opening, this means the territory closest to completing our best bonus.
        // When behind, this means a breakthrough point.
        if let Some(best) = attack_plan.first() {
            best.from
        } else if let Some(&(counter_tid, _)) = counter_targets.first() {
            // Deploy near a counter-expansion target
            map.territories[counter_tid]
                .adjacent
                .iter()
                .copied()
                .find(|&adj| state.territory_owners[adj] == player)
                .unwrap_or_else(|| find_most_threatened_border(state, player, board).unwrap_or(0))
        } else {
            find_most_threatened_border(state, player, board).unwrap_or(0)
        }
    } else if dominant {
        // Spread: deploy to the most threatened border to defend bonuses.
        // But if there's a counter-expansion opportunity, prioritize that.
        if let Some(&(counter_tid, prio)) = counter_targets.first() {
            if prio >= 4.0 {
                // High-value counter target — deploy adjacent to it
                map.territories[counter_tid]
                    .adjacent
                    .iter()
                    .copied()
                    .find(|&adj| state.territory_owners[adj] == player)
                    .unwrap_or_else(|| find_most_threatened_border(state, player, board).unwrap_or(0))
            } else {
                find_most_threatened_border(state, player, board).unwrap_or(0)
            }
        } else {
            find_most_threatened_border(state, player, board).unwrap_or(0)
        }
    } else {
        // Even: stack toward the best attack if we have one, else toward
        // the most valuable incomplete bonus
        if let Some(best) = attack_plan.first() {
            best.from
        } else {
            find_most_threatened_border(state, player, board).unwrap_or_else(|| {
                (0..map.territory_count())
                    .find(|&tid| state.territory_owners[tid] == player)
                    .unwrap_or(0)
            })
        }
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

    // Only attack from territories owned at turn start (no chain attacks).
    let start_owners = state.territory_owners.clone();

    let mut used_sources = vec![false; map.territory_count()];
    // Cap total attack orders to prevent unreasonably long chains (Bug 4).
    let max_attacks = map.territory_count().min(20);
    let mut attack_count = 0usize;

    // Re-evaluate attacks with new army counts
    attack_plan.clear();

    // Rebuild attack list with deployed armies — bonus completion first
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
            // Only attack from territories owned at turn start (no chaining).
            let source = map.territories[target]
                .adjacent
                .iter()
                .copied()
                .filter(|&adj| sim_owners[adj] == player && start_owners[adj] == player && !used_sources[adj])
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
                let result = resolve_attack(attackers, defenders, board.settings());

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

    // Counter-expansion attacks: deny opponent bonuses
    for &(target, _priority) in &counter_targets {
        if attack_count >= max_attacks {
            break;
        }
        if sim_owners[target] == player {
            continue; // Already own it
        }
        // Only attack from territories owned at turn start (no chaining).
        let source = map.territories[target]
            .adjacent
            .iter()
            .copied()
            .filter(|&adj| sim_owners[adj] == player && start_owners[adj] == player && !used_sources[adj])
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
            let result = resolve_attack(attackers, defenders, board.settings());

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
            } else if is_bonus_denial_worthwhile(target, opp, board, state) {
                // Improvement #5: attack even if non-capturable when it weakens
                // a key enemy bonus position
                orders.push(Order::Attack {
                    from: src,
                    to: target,
                    armies: attackers,
                });
                sim_armies[src] = 1;
                // Don't update sim_owners since we didn't capture
                let killed = (attackers as f64 * board.settings().offense_kill_rate).round() as u32;
                sim_armies[target] = sim_armies[target].saturating_sub(killed);
                used_sources[src] = true;
                attack_count += 1;
            }
        }
    }

    // Opportunistic attacks on weak neighbors (raise threshold when dominant)
    // Improvement #5: only attack if capture is possible (or denial is worthwhile)
    let weak_threshold = if dominant || endgame { u32::MAX } else { 3 };
    if !is_opening {
        // Skip opportunistic attacks during opening — stay focused on bonus
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
            // Only attack from territories owned at turn start (no chaining).
            let source = map.territories[tid]
                .adjacent
                .iter()
                .copied()
                .filter(|&adj| {
                    sim_owners[adj] == player && start_owners[adj] == player && !used_sources[adj] && sim_armies[adj] > 1
                })
                .max_by_key(|&adj| sim_armies[adj]);

            if let Some(src) = source {
                let attackers = sim_armies[src] - 1;
                let defenders = sim_armies[tid];
                if defenders == 0 || attackers == 0 {
                    continue;
                }
                let result = resolve_attack(attackers, defenders, board.settings());

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
                // Improvement #5: don't waste armies on futile attacks
                // (non-capturable targets that aren't strategically important)
            }
        }
    }

    // ========== ENDGAME: ATTACK FROM EVERY BORDER ==========
    if endgame {
        for tid in 0..map.territory_count() {
            if attack_count >= max_attacks {
                break;
            }
            // Only attack from territories owned at turn start (no chaining).
            if sim_owners[tid] != player || start_owners[tid] != player || used_sources[tid] || sim_armies[tid] <= 1 {
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
                let result = resolve_attack(attackers, defenders, board.settings());
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

    // ========== TRANSFERS (improvement #3: BFS to most threatened border) ==========
    // Also improvement #6: if no attacks were made, transfer idle armies to
    // stack on the best border for a future attack.
    let had_attacks = orders.iter().any(|o| matches!(o, Order::Attack { .. }));

    for tid in 0..map.territory_count() {
        if sim_owners[tid] != player || sim_armies[tid] <= 1 {
            continue;
        }
        // Skip territories that were used as attack sources
        if used_sources[tid] {
            continue;
        }
        let is_interior = map.territories[tid]
            .adjacent
            .iter()
            .all(|&adj| sim_owners[adj] == player);

        if is_interior {
            // Interior territory: BFS toward the most threatened border
            if let Some(toward) =
                bfs_toward_threatened_border(tid, player, board, &sim_owners, &sim_armies)
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
        } else if !had_attacks {
            // Improvement #6: border territory with no attacks this turn.
            // Transfer toward the best stacking position (border territory
            // with highest enemy threat nearby).
            let best_border = find_best_stack_border(player, board, &sim_owners, &sim_armies);
            if let Some(dest) = best_border {
                if dest != tid {
                    // Only transfer if the destination is adjacent
                    if map.territories[tid].adjacent.contains(&dest) {
                        let amount = sim_armies[tid] - 1;
                        if amount > 0 {
                            orders.push(Order::Transfer {
                                from: tid,
                                to: dest,
                                armies: amount,
                            });
                            sim_armies[tid] = 1;
                            sim_armies[dest] += amount;
                        }
                    } else {
                        // Not adjacent — BFS one step toward the best border
                        if let Some(toward) = bfs_toward_target(tid, dest, board, player, &sim_owners)
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
                }
            }
        }
    }

    orders
}

/// Cleanup mode: spread deployment across all border territories, attack every
/// capturable target, and transfer interior armies toward the front.
fn generate_cleanup_orders(state: &GameState, player: PlayerId, board: &Board) -> Vec<Order> {
    let map = &board.map;
    let mut orders = Vec::new();
    let income = state.income(player, board);
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
    // Only attack from territories owned at turn start (no chaining).
    let start_owners = state.territory_owners.clone();
    let max_attacks = map.territory_count().min(30);
    let mut attack_count = 0usize;
    let mut used_sources = vec![false; map.territory_count()];

    for &tid in &border_territories {
        if attack_count >= max_attacks {
            break;
        }
        if start_owners[tid] != player || used_sources[tid] || sim_armies[tid] <= 1 {
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
            let result = resolve_attack(attackers, defenders, board.settings());
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
        if let Some(&toward) = map.territories[tid].adjacent.iter().max_by_key(|&&adj| {
            map.territories[adj]
                .adjacent
                .iter()
                .filter(|&&a2| sim_owners[a2] != player)
                .count()
        }) {
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
fn find_best_endgame_deploy(state: &GameState, player: PlayerId, board: &Board) -> Option<usize> {
    let map = &board.map;
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
fn find_most_threatened_border(state: &GameState, player: PlayerId, board: &Board) -> Option<usize> {
    let map = &board.map;
    let mut best = None;
    let mut best_threat = 0u32;
    for tid in 0..map.territory_count() {
        if state.territory_owners[tid] != player {
            continue;
        }
        let threat: u32 = map.territories[tid]
            .adjacent
            .iter()
            .filter(|&&adj| {
                state.territory_owners[adj] != player && state.territory_owners[adj] != NEUTRAL
            })
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

/// Check if attacking a territory is worthwhile for bonus denial, even if
/// capture isn't possible. Returns true if the target is in a bonus the
/// opponent is close to completing.
fn is_bonus_denial_worthwhile(target: usize, opp: PlayerId, board: &Board, state: &GameState) -> bool {
    let map = &board.map;
    let bonus = &map.bonuses[map.territories[target].bonus_id];
    let opp_owned = bonus
        .territory_ids
        .iter()
        .filter(|&&tid| state.territory_owners[tid] == opp)
        .count();
    // Worthwhile if opponent owns most of this bonus
    opp_owned as f64 / bonus.territory_ids.len() as f64 >= 0.5
}

/// BFS from `start` toward the most threatened border territory, returning
/// the first step (neighbor of `start`) along the shortest path.
/// "Most threatened" = border territory facing the highest enemy army total.
fn bfs_toward_threatened_border(
    start: usize,
    player: PlayerId,
    board: &Board,
    sim_owners: &[PlayerId],
    sim_armies: &[u32],
) -> Option<usize> {
    let map = &board.map;
    // Find the most threatened border territory as the BFS target
    let mut best_border = None;
    let mut best_threat = 0u32;
    for tid in 0..map.territory_count() {
        if sim_owners[tid] != player {
            continue;
        }
        let is_border = map.territories[tid]
            .adjacent
            .iter()
            .any(|&adj| sim_owners[adj] != player);
        if !is_border {
            continue;
        }
        let threat: u32 = map.territories[tid]
            .adjacent
            .iter()
            .filter(|&&adj| sim_owners[adj] != player)
            .map(|&adj| sim_armies[adj])
            .sum();
        if threat > best_threat {
            best_threat = threat;
            best_border = Some(tid);
        }
    }

    let target = best_border?;
    if target == start {
        return None;
    }

    bfs_toward_target(start, target, board, player, sim_owners)
}

/// BFS from `start` to `target` through friendly territory, returning the
/// first step (neighbor of `start`) along the shortest path.
fn bfs_toward_target(
    start: usize,
    target: usize,
    board: &Board,
    player: PlayerId,
    sim_owners: &[PlayerId],
) -> Option<usize> {
    let map = &board.map;
    // BFS from target backward to start (so we can extract the first step)
    let n = map.territory_count();
    let mut visited = vec![false; n];
    let mut parent = vec![usize::MAX; n];
    let mut queue = VecDeque::new();

    visited[target] = true;
    queue.push_back(target);

    while let Some(cur) = queue.pop_front() {
        if cur == start {
            // parent[start] is the node from which start was discovered in
            // the BFS from target — that node IS one step closer to target.
            return Some(parent[start]);
        }
        for &adj in &map.territories[cur].adjacent {
            if visited[adj] {
                continue;
            }
            // Allow traversal through friendly territory (and the start itself)
            if sim_owners[adj] != player && adj != start {
                continue;
            }
            visited[adj] = true;
            parent[adj] = cur;
            queue.push_back(adj);
        }
    }

    // Fallback: no path found via friendly territory, just pick neighbor
    // closest to any border
    map.territories[start]
        .adjacent
        .iter()
        .copied()
        .filter(|&adj| sim_owners[adj] == player)
        .max_by_key(|&adj| {
            map.territories[adj]
                .adjacent
                .iter()
                .filter(|&&a2| sim_owners[a2] != player)
                .count()
        })
}

/// Find the best border territory to stack armies on: the one facing the
/// highest total enemy armies (best place to build up for a future attack).
fn find_best_stack_border(
    player: PlayerId,
    board: &Board,
    sim_owners: &[PlayerId],
    sim_armies: &[u32],
) -> Option<usize> {
    let map = &board.map;
    let mut best = None;
    let mut best_score = 0i64;
    for tid in 0..map.territory_count() {
        if sim_owners[tid] != player {
            continue;
        }
        let enemy_neighbors: Vec<usize> = map.territories[tid]
            .adjacent
            .iter()
            .copied()
            .filter(|&adj| sim_owners[adj] != player)
            .collect();
        if enemy_neighbors.is_empty() {
            continue; // Interior, not a border
        }
        // Score: prefer territories adjacent to weak enemies (capturable targets)
        // and with more enemy neighbors (attack surface)
        let weakest_enemy: u32 = enemy_neighbors
            .iter()
            .map(|&adj| sim_armies[adj])
            .min()
            .unwrap_or(u32::MAX);
        let score = (enemy_neighbors.len() as i64) * 100 - weakest_enemy as i64;
        if score > best_score || best.is_none() {
            best_score = score;
            best = Some(tid);
        }
    }
    best
}

/// Generate picks for the AI. Prefers territories in small, high-value bonuses.
/// Only picks from the same `pick_options` available to the human player.
pub fn generate_picks(state: &GameState, board: &Board, pick_options: &[usize]) -> Vec<usize> {
    let map = &board.map;
    let mut scored: Vec<(usize, f64)> = pick_options
        .iter()
        .copied()
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
