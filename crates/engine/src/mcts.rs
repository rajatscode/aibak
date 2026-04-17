//! Monte Carlo Tree Search (MCTS) AI for order generation.
//!
//! Uses UCB1 for tree selection, the greedy AI for rollout simulation,
//! and a heuristic board evaluation at leaf nodes.

use std::time::{Duration, Instant};

use rand::rngs::SmallRng;
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};

use crate::ai;
use crate::combat::resolve_attack;
use crate::board::Board;
use crate::orders::Order;
use crate::state::{GameState, NEUTRAL, Phase, PlayerId};
use crate::turn::resolve_turn;

/// Configuration for the MCTS search.
#[derive(Debug, Clone)]
pub struct MctsConfig {
    /// How long the AI is allowed to think.
    pub time_budget: Duration,
    /// UCB1 exploration constant (default sqrt(2) ~= 1.41).
    pub exploration_constant: f64,
    /// Maximum rollout depth in turns (default 10).
    pub max_rollout_depth: u32,
}

impl Default for MctsConfig {
    fn default() -> Self {
        Self {
            time_budget: Duration::from_millis(500),
            exploration_constant: 1.41,
            max_rollout_depth: 10,
        }
    }
}

/// A candidate set of orders that can be chosen at one decision point.
#[derive(Debug, Clone)]
struct MctsAction {
    orders: Vec<Order>,
    #[allow(dead_code)]
    label: String,
}

/// A node in the MCTS tree (one per action choice at a given state).
struct MctsNode {
    action_index: usize, // index into the parent's action list
    visits: u32,
    total_value: f64,
    children: Vec<MctsNode>,
    expanded: bool,
}

impl MctsNode {
    fn new(action_index: usize) -> Self {
        Self {
            action_index,
            visits: 0,
            total_value: 0.0,
            children: Vec::new(),
            expanded: false,
        }
    }

    fn avg_value(&self) -> f64 {
        if self.visits == 0 {
            0.0
        } else {
            self.total_value / self.visits as f64
        }
    }

    fn ucb1(&self, parent_visits: u32, c: f64) -> f64 {
        if self.visits == 0 {
            f64::INFINITY
        } else {
            self.avg_value() + c * ((parent_visits as f64).ln() / self.visits as f64).sqrt()
        }
    }
}

/// Generate orders using Monte Carlo Tree Search.
///
/// The approach:
/// 1. Generate a set of candidate order sets (actions) using heuristic variations.
/// 2. Build a search tree where each node represents choosing one action.
/// 3. For rollouts, simulate both players using the greedy AI.
/// 4. Evaluate leaf positions using a heuristic.
pub fn mcts_generate_orders(
    state: &GameState,
    player: PlayerId,
    board: &Board,
    config: &MctsConfig,
) -> Vec<Order> {
    // If the game is not in play phase, fall back to greedy.
    if state.phase != Phase::Play {
        return ai::generate_orders(state, player, board);
    }

    // If the player has no territories, nothing to do.
    if state.territory_count_for(player) == 0 {
        return Vec::new();
    }

    let mut rng = SmallRng::from_entropy();

    // Generate candidate actions (diverse order sets).
    let actions = generate_candidate_actions(state, player, board, &mut rng);

    if actions.is_empty() {
        return ai::generate_orders(state, player, board);
    }

    if actions.len() == 1 {
        return actions[0].orders.clone();
    }

    // Build root node.
    let mut root = MctsNode::new(0);
    root.expanded = true;
    for i in 0..actions.len() {
        root.children.push(MctsNode::new(i));
    }

    let start = Instant::now();
    let mut iterations = 0u32;

    while start.elapsed() < config.time_budget {
        // Select child with best UCB1.
        let child_idx = select_child(&root, config.exploration_constant);

        // Simulate from this action.
        let value = simulate(
            state,
            player,
            board,
            &actions[root.children[child_idx].action_index].orders,
            config.max_rollout_depth,
            &mut rng,
        );

        // Backpropagate.
        root.visits += 1;
        root.children[child_idx].visits += 1;
        root.children[child_idx].total_value += value;

        iterations += 1;
    }

    // Choose action with most visits (robust child selection).
    let best_child = root.children.iter().max_by_key(|c| c.visits).unwrap();

    let _best_action = &actions[best_child.action_index];
    tracing_log(iterations, &actions, &root);

    actions[best_child.action_index].orders.clone()
}

fn tracing_log(_iterations: u32, _actions: &[MctsAction], _root: &MctsNode) {
    // MCTS statistics available for debugging if needed.
}

/// Select child index with highest UCB1 value.
fn select_child(parent: &MctsNode, c: f64) -> usize {
    let parent_visits = parent.visits.max(1);
    parent
        .children
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| {
            a.ucb1(parent_visits, c)
                .partial_cmp(&b.ucb1(parent_visits, c))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(i, _)| i)
        .unwrap_or(0)
}

/// Simulate a game from the given state after applying the chosen orders.
/// Returns a value in [0, 1] representing how good the outcome is for `player`.
fn simulate(
    state: &GameState,
    player: PlayerId,
    board: &Board,
    player_orders: &[Order],
    max_depth: u32,
    rng: &mut impl Rng,
) -> f64 {
    let opponent = 1 - player;

    // Generate opponent orders using greedy AI.
    let opp_orders = ai::generate_orders(state, opponent, board);

    // Build the order arrays.
    let mut orders: [Vec<Order>; 2] = [Vec::new(), Vec::new()];
    orders[player as usize] = player_orders.to_vec();
    orders[opponent as usize] = opp_orders;

    // Resolve the first turn with our chosen orders.
    let result = resolve_turn(state, orders, board, rng);
    let mut current = result.state;

    // Continue simulation using greedy AI for both sides.
    for _ in 1..max_depth {
        if current.phase == Phase::Finished {
            break;
        }
        if current.phase != Phase::Play {
            break;
        }

        let p0_orders = ai::generate_orders(&current, 0, board);
        let p1_orders = ai::generate_orders(&current, 1, board);
        let result = resolve_turn(&current, [p0_orders, p1_orders], board, rng);
        current = result.state;
    }

    // Evaluate the final position.
    evaluate_position(&current, player, board)
}

/// Evaluate a game position from `player`'s perspective. Returns [0, 1].
pub fn evaluate_position(state: &GameState, player: PlayerId, board: &Board) -> f64 {
    let map = &board.map;
    let opponent = 1 - player;

    // Terminal states.
    if state.phase == Phase::Finished {
        return if state.winner == Some(player) {
            1.0
        } else {
            0.0
        };
    }

    let my_territories = state.territory_count_for(player) as f64;
    let opp_territories = state.territory_count_for(opponent) as f64;

    if my_territories == 0.0 {
        return 0.0;
    }
    if opp_territories == 0.0 {
        return 1.0;
    }

    // Territory ratio [0, 1] — our share vs opponent (ignoring neutrals).
    let territory_score = my_territories / (my_territories + opp_territories);

    // Income comparison.
    let my_income = state.income(player, board) as f64;
    let opp_income = state.income(opponent, board) as f64;
    let income_score = my_income / (my_income + opp_income).max(1.0);

    // Army count ratio.
    let my_armies: u32 = (0..map.territory_count())
        .filter(|&t| state.territory_owners[t] == player)
        .map(|t| state.territory_armies[t])
        .sum();
    let opp_armies: u32 = (0..map.territory_count())
        .filter(|&t| state.territory_owners[t] == opponent)
        .map(|t| state.territory_armies[t])
        .sum();
    let army_score = my_armies as f64 / (my_armies + opp_armies).max(1) as f64;

    // Border strength: ratio of our border armies to enemy border armies.
    let border_score = border_strength_score(state, player, board);

    // Bonus completion proximity (linear).
    let bonus_score = bonus_proximity_score(state, player, board)
        - bonus_proximity_score(state, opponent, board) * 0.5;
    let bonus_score_norm = (bonus_score + 1.0) / 2.0;

    // Weighted combination — territory control is king.
    let mut raw = territory_score * 0.35
        + income_score * 0.25
        + army_score * 0.15
        + border_score * 0.15
        + bonus_score_norm * 0.10;

    // Finishing bonus: when already dominating (>60% of contested territory),
    // add extra incentive proportional to how close we are to total elimination.
    // This prevents the AI from stalling when it should be closing out the game.
    if territory_score > 0.6 {
        raw += (territory_score - 0.6) * 0.5;
    }

    raw.clamp(0.01, 0.99)
}

/// Score how close a player is to completing bonuses. Higher is better.
fn bonus_proximity_score(state: &GameState, player: PlayerId, board: &Board) -> f64 {
    let map = &board.map;
    let mut score = 0.0;
    for bonus in &map.bonuses {
        if bonus.value == 0 {
            continue;
        }
        let owned = bonus
            .territory_ids
            .iter()
            .filter(|&&tid| state.territory_owners[tid] == player)
            .count();
        let total = bonus.territory_ids.len();
        if owned == total {
            // Already completed.
            score += bonus.value as f64 * 0.5;
        } else if owned > 0 {
            let completion = owned as f64 / total as f64;
            let efficiency = bonus.value as f64 / total as f64;
            score += completion * efficiency;
        }
    }
    // Normalize by total possible bonus value.
    let max_bonus_value: u32 = map.bonuses.iter().map(|b| b.value).sum();
    if max_bonus_value > 0 {
        score / max_bonus_value as f64
    } else {
        0.0
    }
}

/// Compute border strength score [0, 1].
/// Ratio of our border armies to total border armies (ours + enemy's).
fn border_strength_score(state: &GameState, player: PlayerId, board: &Board) -> f64 {
    let map = &board.map;
    let mut my_border_armies = 0u32;
    let mut enemy_border_armies = 0u32;
    let mut counted_enemies = vec![false; map.territory_count()];

    for tid in 0..map.territory_count() {
        if state.territory_owners[tid] != player {
            continue;
        }
        let mut is_border = false;
        for &adj in &map.territories[tid].adjacent {
            if state.territory_owners[adj] != player && state.territory_owners[adj] != NEUTRAL {
                is_border = true;
                if !counted_enemies[adj] {
                    counted_enemies[adj] = true;
                    enemy_border_armies += state.territory_armies[adj];
                }
            }
        }
        if is_border {
            my_border_armies += state.territory_armies[tid];
        }
    }

    let total = my_border_armies + enemy_border_armies;
    if total == 0 {
        0.5
    } else {
        my_border_armies as f64 / total as f64
    }
}

// ── Candidate action generation ──

/// Generate diverse candidate order sets for MCTS to choose between.
fn generate_candidate_actions(
    state: &GameState,
    player: PlayerId,
    board: &Board,
    rng: &mut impl Rng,
) -> Vec<MctsAction> {
    let map = &board.map;
    let mut actions = Vec::new();
    let income = state.income(player, board);
    if income == 0 {
        return actions;
    }

    // Action 0: Greedy AI baseline.
    let greedy_orders = ai::generate_orders(state, player, board);
    if !greedy_orders.is_empty() {
        actions.push(MctsAction {
            orders: greedy_orders,
            label: "greedy".into(),
        });
    }

    // Find border territories (owned, adjacent to non-owned).
    let border_territories: Vec<usize> = (0..map.territory_count())
        .filter(|&tid| {
            state.territory_owners[tid] == player
                && map.territories[tid]
                    .adjacent
                    .iter()
                    .any(|&adj| state.territory_owners[adj] != player)
        })
        .collect();

    if border_territories.is_empty() {
        return actions;
    }

    // Generate several deployment variations with different attack plans.
    for variation in 0..12 {
        let orders = generate_variation(
            state,
            player,
            board,
            &border_territories,
            income,
            variation,
            rng,
        );
        if !orders.is_empty() {
            let label = format!("var_{}", variation);
            // Avoid duplicates (same deploy target).
            let dominated = actions.iter().any(|a: &MctsAction| a.orders == orders);
            if !dominated {
                actions.push(MctsAction { orders, label });
            }
        }
    }

    // Defensive variation: deploy to the most threatened border.
    if let Some(orders) =
        generate_defensive_variation(state, player, board, &border_territories, income)
        && !actions.iter().any(|a| a.orders == orders)
    {
        actions.push(MctsAction {
            orders,
            label: "defensive".into(),
        });
    }

    // Hold variation: deploy only, no attacks (sometimes holding is best).
    // Skip when dominating — holding wastes the advantage.
    let my_territories = state.territory_count_for(player) as f64;
    let opp_territories = state.territory_count_for(1 - player) as f64;
    let territory_ratio = my_territories / (my_territories + opp_territories).max(1.0);

    if territory_ratio <= 0.6 {
        if let Some(orders) =
            generate_hold_variation(state, player, board, &border_territories, income)
            && !actions.iter().any(|a| a.orders == orders)
        {
            actions.push(MctsAction {
                orders,
                label: "hold".into(),
            });
        }
    }

    // Attack only the single weakest neighbor.
    if let Some(orders) =
        generate_attack_weakest_variation(state, player, board, &border_territories, income)
        && !actions.iter().any(|a| a.orders == orders)
    {
        actions.push(MctsAction {
            orders,
            label: "attack_weakest".into(),
        });
    }

    // Bonus focus: deploy and attack to complete a specific bonus.
    if let Some(orders) =
        generate_bonus_focus_variation(state, player, board, &border_territories, income)
        && !actions.iter().any(|a| a.orders == orders)
    {
        actions.push(MctsAction {
            orders,
            label: "bonus_focus".into(),
        });
    }

    // All-out attack: when dominating (>60% territory), generate an aggressive
    // variation that deploys to the strongest border and attacks everything.
    if territory_ratio > 0.6 {
        if let Some(orders) =
            generate_all_out_attack_variation(state, player, board, &border_territories, income)
            && !actions.iter().any(|a| a.orders == orders)
        {
            actions.push(MctsAction {
                orders,
                label: "all_out_attack".into(),
            });
        }
    }

    actions
}

/// Generate a single variation of orders.
fn generate_variation(
    state: &GameState,
    player: PlayerId,
    board: &Board,
    border_territories: &[usize],
    income: u32,
    variation: usize,
    rng: &mut impl Rng,
) -> Vec<Order> {
    let map = &board.map;
    let mut orders = Vec::new();
    let opponent = 1 - player;

    // Pick a deployment target based on variation.
    let deploy_target = match variation {
        0 => {
            // Deploy to border territory adjacent to weakest enemy.
            border_territories.iter().copied().min_by_key(|&tid| {
                map.territories[tid]
                    .adjacent
                    .iter()
                    .filter(|&&adj| state.territory_owners[adj] != player)
                    .map(|&adj| state.territory_armies[adj])
                    .min()
                    .unwrap_or(u32::MAX)
            })
        }
        1 => {
            // Deploy to territory adjacent to highest-value uncompleted bonus.
            best_bonus_border(state, player, board, border_territories)
        }
        2 => {
            // Deploy to most threatened border.
            border_territories.iter().copied().max_by_key(|&tid| {
                map.territories[tid]
                    .adjacent
                    .iter()
                    .filter(|&&adj| state.territory_owners[adj] == opponent)
                    .map(|&adj| state.territory_armies[adj])
                    .sum::<u32>()
            })
        }
        3 => {
            // Deploy to border with most enemy neighbors (maximize attack options).
            border_territories.iter().copied().max_by_key(|&tid| {
                map.territories[tid]
                    .adjacent
                    .iter()
                    .filter(|&&adj| state.territory_owners[adj] != player)
                    .count()
            })
        }
        4 => {
            // Deploy to territory with most existing armies (stack).
            border_territories
                .iter()
                .copied()
                .max_by_key(|&tid| state.territory_armies[tid])
        }
        5 => {
            // Deploy adjacent to a territory that completes a bonus (1-away).
            one_away_bonus_border(state, player, board, border_territories)
                .or_else(|| best_bonus_border(state, player, board, border_territories))
        }
        6 => {
            // Deploy to weakest border territory (shore up weakness).
            border_territories
                .iter()
                .copied()
                .min_by_key(|&tid| state.territory_armies[tid])
        }
        7 => {
            // Deploy to border territory in our most complete partial bonus.
            let mut best = None;
            let mut best_frac = 0.0f64;
            for bonus in &map.bonuses {
                if bonus.value == 0 { continue; }
                let total = bonus.territory_ids.len();
                let owned = bonus.territory_ids.iter().filter(|&&t| state.territory_owners[t] == player).count();
                if owned == 0 || owned == total { continue; }
                let frac = owned as f64 / total as f64;
                if frac > best_frac {
                    // Find a border territory in or adjacent to this bonus.
                    let missing: Vec<usize> = bonus.territory_ids.iter().copied()
                        .filter(|&t| state.territory_owners[t] != player).collect();
                    for &bt in border_territories {
                        if missing.iter().any(|&m| map.are_adjacent(bt, m)) {
                            best_frac = frac;
                            best = Some(bt);
                            break;
                        }
                    }
                }
            }
            best
        }
        _ => {
            // Random border territory (diversity).
            border_territories.choose(rng).copied()
        }
    };

    let deploy_target = match deploy_target {
        Some(t) => t,
        None => return Vec::new(),
    };

    orders.push(Order::Deploy {
        territory: deploy_target,
        armies: income,
    });

    // Simulate deployment and generate attacks.
    let mut sim_armies = state.territory_armies.clone();
    let mut sim_owners = state.territory_owners.clone();
    let start_owners = state.territory_owners.clone();
    sim_armies[deploy_target] += income;

    // Attack from deploy target first.
    generate_attacks_from(
        deploy_target,
        player,
        board,
        &mut sim_armies,
        &mut sim_owners,
        &mut orders,
        &start_owners,
    );

    // Then try attacks from other border territories.
    for &tid in border_territories {
        if tid == deploy_target {
            continue;
        }
        generate_attacks_from(
            tid,
            player,
            board,
            &mut sim_armies,
            &mut sim_owners,
            &mut orders,
            &start_owners,
        );
    }

    // Generate transfers for interior armies.
    generate_transfers(player, board, &mut sim_armies, &sim_owners, &mut orders);

    orders
}

/// Generate attacks from a single territory.
/// `start_owners` is the ownership snapshot at turn start — only attack from
/// territories the player owned before any mid-turn captures.
fn generate_attacks_from(
    from: usize,
    player: PlayerId,
    board: &Board,
    sim_armies: &mut [u32],
    sim_owners: &mut [PlayerId],
    orders: &mut Vec<Order>,
    start_owners: &[PlayerId],
) {
    let map = &board.map;
    // Only attack from territories owned at turn start (no chaining).
    if start_owners[from] != player || sim_owners[from] != player || sim_armies[from] <= 1 {
        return;
    }

    // Find attackable neighbors, sorted by army count (weakest first).
    let mut targets: Vec<usize> = map.territories[from]
        .adjacent
        .iter()
        .copied()
        .filter(|&adj| sim_owners[adj] != player)
        .collect();
    targets.sort_by_key(|&t| sim_armies[t]);

    for target in targets {
        if sim_armies[from] <= 1 {
            break;
        }
        let attackers = sim_armies[from] - 1;
        let defenders = sim_armies[target];
        if defenders == 0 || attackers == 0 {
            continue;
        }
        let result = resolve_attack(attackers, defenders, board.settings());

        if result.captured {
            orders.push(Order::Attack {
                from,
                to: target,
                armies: attackers,
            });
            sim_armies[from] = 1;
            sim_armies[target] = result.surviving_attackers;
            sim_owners[target] = player;
        }
    }
}

/// Generate transfer orders to move interior armies toward the front.
fn generate_transfers(
    player: PlayerId,
    board: &Board,
    sim_armies: &mut [u32],
    sim_owners: &[PlayerId],
    orders: &mut Vec<Order>,
) {
    let map = &board.map;
    for tid in 0..map.territory_count() {
        if sim_owners[tid] != player || sim_armies[tid] <= 1 {
            continue;
        }
        let is_interior = map.territories[tid]
            .adjacent
            .iter()
            .all(|&adj| sim_owners[adj] == player);
        if !is_interior {
            continue;
        }

        // Transfer toward the neighbor closest to the front.
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
            }
        }
    }
}

/// Find the border territory best positioned to complete a high-value bonus.
fn best_bonus_border(
    state: &GameState,
    player: PlayerId,
    board: &Board,
    border_territories: &[usize],
) -> Option<usize> {
    let map = &board.map;
    let mut best_tid = None;
    let mut best_score = -1.0f64;

    for bonus in &map.bonuses {
        if bonus.value == 0 {
            continue;
        }
        let owned = bonus
            .territory_ids
            .iter()
            .filter(|&&tid| state.territory_owners[tid] == player)
            .count();
        let total = bonus.territory_ids.len();
        if owned == 0 || owned == total {
            continue;
        }

        let completion = owned as f64 / total as f64;
        let efficiency = bonus.value as f64 / total as f64;
        let score = completion * efficiency;

        // Find a border territory in this bonus or adjacent to missing territories.
        let missing: Vec<usize> = bonus
            .territory_ids
            .iter()
            .copied()
            .filter(|&tid| state.territory_owners[tid] != player)
            .collect();

        for &bt in border_territories {
            let is_adjacent_to_missing = missing.iter().any(|&m| map.are_adjacent(bt, m));
            if is_adjacent_to_missing && score > best_score {
                best_score = score;
                best_tid = Some(bt);
            }
        }
    }

    best_tid.or_else(|| border_territories.first().copied())
}

/// Generate a purely defensive set of orders.
fn generate_defensive_variation(
    state: &GameState,
    player: PlayerId,
    board: &Board,
    border_territories: &[usize],
    income: u32,
) -> Option<Vec<Order>> {
    let map = &board.map;
    let opponent = 1 - player;

    // Find the most threatened border territory.
    let deploy_target = border_territories.iter().copied().max_by_key(|&tid| {
        map.territories[tid]
            .adjacent
            .iter()
            .filter(|&&adj| state.territory_owners[adj] == opponent)
            .map(|&adj| state.territory_armies[adj])
            .sum::<u32>()
    })?;

    let mut orders = vec![Order::Deploy {
        territory: deploy_target,
        armies: income,
    }];

    // Only attack very weak neighbors.
    let mut sim_armies = state.territory_armies.clone();
    let sim_owners = state.territory_owners.clone();
    sim_armies[deploy_target] += income;

    for &adj in &map.territories[deploy_target].adjacent {
        if sim_owners[adj] == player || sim_armies[deploy_target] <= 1 {
            continue;
        }
        // Only attack if we have overwhelming force (3:1 ratio).
        let attackers = sim_armies[deploy_target] - 1;
        let defenders = sim_armies[adj];
        if attackers >= defenders * 3 {
            let result = resolve_attack(attackers, defenders, board.settings());
            if result.captured {
                orders.push(Order::Attack {
                    from: deploy_target,
                    to: adj,
                    armies: attackers,
                });
                sim_armies[deploy_target] = 1;
                break;
            }
        }
    }

    // Transfer interior armies toward deploy_target.
    let mut sim_armies_t = sim_armies.clone();
    generate_transfers(player, board, &mut sim_armies_t, &sim_owners, &mut orders);

    Some(orders)
}

/// Find the border territory adjacent to a bonus missing exactly 1 territory.
fn one_away_bonus_border(
    state: &GameState,
    player: PlayerId,
    board: &Board,
    border_territories: &[usize],
) -> Option<usize> {
    let map = &board.map;
    let mut best_tid = None;
    let mut best_value = 0u32;

    for bonus in &map.bonuses {
        if bonus.value == 0 {
            continue;
        }
        let total = bonus.territory_ids.len();
        let owned = bonus
            .territory_ids
            .iter()
            .filter(|&&tid| state.territory_owners[tid] == player)
            .count();
        // Missing exactly 1 territory.
        if owned + 1 != total {
            continue;
        }
        let missing = match bonus
            .territory_ids
            .iter()
            .copied()
            .find(|&tid| state.territory_owners[tid] != player)
        {
            Some(m) => m,
            None => continue,
        };
        for &bt in border_territories {
            if map.are_adjacent(bt, missing) && bonus.value > best_value {
                best_value = bonus.value;
                best_tid = Some(bt);
            }
        }
    }

    best_tid
}

/// Generate a hold variation: deploy but make no attacks.
fn generate_hold_variation(
    state: &GameState,
    player: PlayerId,
    board: &Board,
    border_territories: &[usize],
    income: u32,
) -> Option<Vec<Order>> {
    let map = &board.map;
    let opponent = 1 - player;

    // Deploy to most threatened border.
    let deploy_target = border_territories.iter().copied().max_by_key(|&tid| {
        map.territories[tid]
            .adjacent
            .iter()
            .filter(|&&adj| state.territory_owners[adj] == opponent)
            .map(|&adj| state.territory_armies[adj])
            .sum::<u32>()
    })?;

    let mut orders = vec![Order::Deploy {
        territory: deploy_target,
        armies: income,
    }];

    // Transfers only, no attacks.
    let mut sim_armies = state.territory_armies.clone();
    sim_armies[deploy_target] += income;
    generate_transfers(
        player,
        board,
        &mut sim_armies,
        &state.territory_owners,
        &mut orders,
    );

    Some(orders)
}

/// Generate a variation that only attacks the single weakest neighbor.
fn generate_attack_weakest_variation(
    state: &GameState,
    player: PlayerId,
    board: &Board,
    border_territories: &[usize],
    income: u32,
) -> Option<Vec<Order>> {
    let map = &board.map;

    // Find the weakest enemy territory adjacent to any of our borders.
    let mut best_from = None;
    let mut best_target = None;
    let mut min_defenders = u32::MAX;

    for &bt in border_territories {
        for &adj in &map.territories[bt].adjacent {
            if state.territory_owners[adj] != player
                && state.territory_owners[adj] != NEUTRAL
                && state.territory_armies[adj] < min_defenders
            {
                min_defenders = state.territory_armies[adj];
                best_from = Some(bt);
                best_target = Some(adj);
            }
        }
    }

    let from = best_from?;
    let target = best_target?;

    let mut orders = vec![Order::Deploy {
        territory: from,
        armies: income,
    }];

    let attackers = state.territory_armies[from] + income - 1;
    let defenders = state.territory_armies[target];
    if attackers > 0 && defenders > 0 {
        let result = resolve_attack(attackers, defenders, board.settings());
        if result.captured {
            orders.push(Order::Attack {
                from,
                to: target,
                armies: attackers,
            });
        }
    }

    let mut sim_armies = state.territory_armies.clone();
    sim_armies[from] += income;
    generate_transfers(
        player,
        board,
        &mut sim_armies,
        &state.territory_owners,
        &mut orders,
    );

    Some(orders)
}

/// Generate a variation focused on completing a specific bonus.
fn generate_bonus_focus_variation(
    state: &GameState,
    player: PlayerId,
    board: &Board,
    border_territories: &[usize],
    income: u32,
) -> Option<Vec<Order>> {
    let map = &board.map;

    // Find the best bonus to complete: high value, few territories missing.
    let mut best_bonus_idx = None;
    let mut best_score = -1.0f64;

    for (idx, bonus) in map.bonuses.iter().enumerate() {
        if bonus.value == 0 {
            continue;
        }
        let total = bonus.territory_ids.len();
        let owned = bonus
            .territory_ids
            .iter()
            .filter(|&&tid| state.territory_owners[tid] == player)
            .count();
        if owned == 0 || owned == total {
            continue;
        }
        let missing = total - owned;
        // Score: bonus value / missing territories (efficiency of completing it).
        let score = bonus.value as f64 / missing as f64;
        if score > best_score {
            best_score = score;
            best_bonus_idx = Some(idx);
        }
    }

    let bonus_idx = best_bonus_idx?;
    let bonus = &map.bonuses[bonus_idx];
    let missing: Vec<usize> = bonus
        .territory_ids
        .iter()
        .copied()
        .filter(|&tid| state.territory_owners[tid] != player)
        .collect();

    // Find the best border territory to deploy to for attacking missing territories.
    let deploy_target = border_territories
        .iter()
        .copied()
        .filter(|&bt| missing.iter().any(|&m| map.are_adjacent(bt, m)))
        .max_by_key(|&bt| state.territory_armies[bt])?;

    let mut orders = vec![Order::Deploy {
        territory: deploy_target,
        armies: income,
    }];

    // Attack only the missing bonus territories.
    let mut sim_armies = state.territory_armies.clone();
    let mut sim_owners = state.territory_owners.clone();
    sim_armies[deploy_target] += income;

    let mut targets: Vec<usize> = map.territories[deploy_target]
        .adjacent
        .iter()
        .copied()
        .filter(|&adj| missing.contains(&adj))
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
        let result = resolve_attack(attackers, defenders, board.settings());
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

    generate_transfers(player, board, &mut sim_armies, &sim_owners, &mut orders);

    Some(orders)
}

/// Generate an all-out attack variation: deploy to the border with the most
/// attackable enemy neighbors and attack everything possible from every border.
fn generate_all_out_attack_variation(
    state: &GameState,
    player: PlayerId,
    board: &Board,
    border_territories: &[usize],
    income: u32,
) -> Option<Vec<Order>> {
    let map = &board.map;

    // Deploy to the border territory with the most enemy neighbors.
    let deploy_target = border_territories.iter().copied().max_by_key(|&tid| {
        let enemy_count = map.territories[tid]
            .adjacent
            .iter()
            .filter(|&&adj| state.territory_owners[adj] != player)
            .count();
        // Prefer territories where we already have armies (can overwhelm).
        (enemy_count, state.territory_armies[tid])
    })?;

    let mut orders = vec![Order::Deploy {
        territory: deploy_target,
        armies: income,
    }];

    let mut sim_armies = state.territory_armies.clone();
    let mut sim_owners = state.territory_owners.clone();
    let start_owners = state.territory_owners.clone();
    sim_armies[deploy_target] += income;

    // Attack from deploy target first, then all other borders.
    generate_attacks_from(
        deploy_target,
        player,
        board,
        &mut sim_armies,
        &mut sim_owners,
        &mut orders,
        &start_owners,
    );
    for &tid in border_territories {
        if tid == deploy_target {
            continue;
        }
        generate_attacks_from(
            tid,
            player,
            board,
            &mut sim_armies,
            &mut sim_owners,
            &mut orders,
            &start_owners,
        );
    }

    // Move all interior armies toward the front.
    generate_transfers(player, board, &mut sim_armies, &sim_owners, &mut orders);

    Some(orders)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::Board;
    use crate::map::{Bonus, MapFile, MapSettings, PickingConfig, PickingMethod, Territory};

    fn test_map() -> MapFile {
        MapFile {
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

    fn setup_play_state(board: &Board) -> GameState {
        let mut state = GameState::new(board);
        state.territory_owners = vec![0, 0, 1, 1];
        state.territory_armies = vec![5, 3, 3, 5];
        state.phase = Phase::Play;
        state.turn = 1;
        state
    }

    #[test]
    fn test_mcts_generates_valid_orders() {
        let map = test_map();
        let board = Board::from_map(map);
        let state = setup_play_state(&board);
        let config = MctsConfig {
            time_budget: Duration::from_millis(100),
            ..Default::default()
        };

        let orders = mcts_generate_orders(&state, 0, &board, &config);

        // Should have at least a deploy order.
        assert!(!orders.is_empty());

        // Verify deploy amount equals income.
        let total_deployed: u32 = orders
            .iter()
            .filter_map(|o| match o {
                Order::Deploy { armies, .. } => Some(*armies),
                _ => None,
            })
            .sum();
        assert_eq!(total_deployed, state.income(0, &board));

        // All deploy targets should be owned by player 0.
        for order in &orders {
            if let Order::Deploy { territory, .. } = order {
                assert_eq!(state.territory_owners[*territory], 0);
            }
        }
    }

    #[test]
    fn test_mcts_generates_orders_for_player_1() {
        let map = test_map();
        let board = Board::from_map(map);
        let state = setup_play_state(&board);
        let config = MctsConfig {
            time_budget: Duration::from_millis(50),
            ..Default::default()
        };

        let orders = mcts_generate_orders(&state, 1, &board, &config);
        assert!(!orders.is_empty());

        let total_deployed: u32 = orders
            .iter()
            .filter_map(|o| match o {
                Order::Deploy { armies, .. } => Some(*armies),
                _ => None,
            })
            .sum();
        assert_eq!(total_deployed, state.income(1, &board));
    }

    #[test]
    fn test_evaluate_position_winning() {
        let map = test_map();
        let board = Board::from_map(map);
        let mut state = setup_play_state(&board);
        // Player 0 owns 3/4 territories with more armies.
        state.territory_owners = vec![0, 0, 0, 1];
        state.territory_armies = vec![5, 5, 5, 1];

        let score = evaluate_position(&state, 0, &board);
        assert!(
            score > 0.5,
            "winning position should score > 0.5, got {}",
            score
        );
    }

    #[test]
    fn test_evaluate_position_losing() {
        let map = test_map();
        let board = Board::from_map(map);
        let mut state = setup_play_state(&board);
        state.territory_owners = vec![0, 1, 1, 1];
        state.territory_armies = vec![1, 5, 5, 5];

        let score = evaluate_position(&state, 0, &board);
        assert!(
            score < 0.5,
            "losing position should score < 0.5, got {}",
            score
        );
    }

    #[test]
    fn test_evaluate_finished_game() {
        let map = test_map();
        let board = Board::from_map(map);
        let mut state = setup_play_state(&board);
        state.phase = Phase::Finished;
        state.winner = Some(0);

        assert_eq!(evaluate_position(&state, 0, &board), 1.0);
        assert_eq!(evaluate_position(&state, 1, &board), 0.0);
    }

    #[test]
    fn test_candidate_actions_generated() {
        let map = test_map();
        let board = Board::from_map(map);
        let state = setup_play_state(&board);
        let mut rng = SmallRng::seed_from_u64(42);

        let actions = generate_candidate_actions(&state, 0, &board, &mut rng);
        // Should have at least one candidate action.
        assert!(!actions.is_empty(), "expected at least 1 action, got 0");
    }
}
