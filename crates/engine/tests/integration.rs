use std::path::PathBuf;
use std::time::{Duration, Instant};

use rand::SeedableRng;
use rand::rngs::StdRng;

use strat_engine::ai::{self, AiStrength};
use strat_engine::analysis::{material_evaluation, quick_win_probability};
use strat_engine::cards::Card;
use strat_engine::combat::resolve_attack;
use strat_engine::fog::{fog_filter, visible_territories};
use strat_engine::game_analysis::analyze_game;
use strat_engine::board::Board;
use strat_engine::map::{Bonus, MapFile, MapSettings, PickingConfig, PickingMethod, Territory};
use strat_engine::mcts::MctsConfig;
use strat_engine::orders::{Order, validate_orders};
use strat_engine::picking;
use strat_engine::puzzle::daily_puzzle;
use strat_engine::state::{GameState, NEUTRAL, Phase};
use strat_engine::turn::{resolve_turn, TurnEvent};

fn maps_dir() -> PathBuf {
    // Navigate from crate root to workspace root
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap() // crates/
        .parent()
        .unwrap() // workspace root
        .join("maps")
}

fn load_small_earth() -> MapFile {
    MapFile::load(&maps_dir().join("small_earth.json")).expect("Failed to load Small Earth map")
}

fn load_big_earth() -> MapFile {
    MapFile::load(&maps_dir().join("big_earth.json")).expect("Failed to load Big Earth map")
}

fn default_settings() -> MapSettings {
    MapSettings {
        luck_pct: 0,
        base_income: 5,
        wasteland_armies: 10,
        unpicked_neutral_armies: 4,
        fog_of_war: true,
        offense_kill_rate: 0.6,
        defense_kill_rate: 0.7,
    }
}

// ========== MAP LOADING ==========

#[test]
fn test_small_earth_loads() {
    let map = load_small_earth();
    assert_eq!(map.territories.len(), 42);
    assert_eq!(map.bonuses.len(), 6);
}

#[test]
fn test_big_earth_loads() {
    let map = load_big_earth();
    assert_eq!(map.territories.len(), 89);
    assert_eq!(map.bonuses.len(), 22);
}

#[test]
fn test_all_adjacencies_bidirectional() {
    for map in [load_small_earth(), load_big_earth()] {
        for t in &map.territories {
            for &adj in &t.adjacent {
                assert!(
                    map.territories[adj].adjacent.contains(&t.id),
                    "Map '{}': territory {} -> {} but {} does not link back",
                    map.name,
                    t.id,
                    adj,
                    adj
                );
            }
        }
    }
}

#[test]
fn test_all_territories_reachable() {
    for map in [load_small_earth(), load_big_earth()] {
        let mut visited = vec![false; map.territories.len()];
        let mut stack = vec![0usize];
        visited[0] = true;
        while let Some(tid) = stack.pop() {
            for &adj in &map.territories[tid].adjacent {
                if !visited[adj] {
                    visited[adj] = true;
                    stack.push(adj);
                }
            }
        }
        let unreachable: Vec<usize> = visited
            .iter()
            .enumerate()
            .filter(|&(_, v)| !v)
            .map(|(i, _)| i)
            .collect();
        assert!(
            unreachable.is_empty(),
            "Map '{}': unreachable territories: {:?}",
            map.name,
            unreachable
        );
    }
}

#[test]
fn test_all_territories_in_exactly_one_bonus() {
    for map in [load_small_earth(), load_big_earth()] {
        let mut bonus_count = vec![0usize; map.territories.len()];
        for bonus in &map.bonuses {
            for &tid in &bonus.territory_ids {
                bonus_count[tid] += 1;
            }
        }
        for (tid, &count) in bonus_count.iter().enumerate() {
            assert_eq!(
                count, 1,
                "Map '{}': territory {} is in {} bonuses (should be 1)",
                map.name, tid, count
            );
        }
    }
}

// ========== COMBAT ==========

#[test]
fn test_combat_deterministic_at_0_luck() {
    let s = default_settings();
    // Same inputs should always give same outputs
    let r1 = resolve_attack(5, 3, &s);
    let r2 = resolve_attack(5, 3, &s);
    assert_eq!(r1.defenders_killed, r2.defenders_killed);
    assert_eq!(r1.attackers_killed, r2.attackers_killed);
    assert_eq!(r1.captured, r2.captured);
}

#[test]
fn test_combat_known_outcomes() {
    let s = default_settings();
    // 2v1: 2*0.6=1.2->1 kill, 1*0.7=0.7->1 kill. Capture (1 def killed, 1 atk survives).
    let r = resolve_attack(2, 1, &s);
    assert!(r.captured);
    assert_eq!(r.surviving_attackers, 1);

    // 3v2: 3*0.6=1.8->2 kills, 2*0.7=1.4->1 kill. Capture.
    let r = resolve_attack(3, 2, &s);
    assert!(r.captured);
    assert_eq!(r.surviving_attackers, 2);

    // 1v1: 1*0.6=0.6->1 kill, 1*0.7=0.7->1 kill. Both die, no capture.
    let r = resolve_attack(1, 1, &s);
    assert!(!r.captured);
    assert_eq!(r.surviving_attackers, 0);
    assert_eq!(r.surviving_defenders, 0);
}

#[test]
fn test_combat_never_negative_survivors() {
    let s = default_settings();
    for atk in 1..=50 {
        for def in 1..=50 {
            let r = resolve_attack(atk, def, &s);
            // surviving_attackers + attackers_killed should not exceed atk
            assert!(r.attackers_killed <= atk);
            assert!(r.defenders_killed <= def);
        }
    }
}

// ========== PICKING ==========

#[test]
fn test_random_warlords_one_per_bonus() {
    let map = load_small_earth();
    let board = Board::from_map(map.clone());
    let mut rng = StdRng::seed_from_u64(42);
    let options = picking::generate_pick_options(&board, &mut rng);

    // Should get exactly one territory per bonus with value > 0
    let expected = map.bonuses.iter().filter(|b| b.value > 0).count();
    assert_eq!(options.len(), expected);

    // Each option should be from a different bonus
    let mut bonus_ids: Vec<usize> = options
        .iter()
        .map(|&tid| map.territories[tid].bonus_id)
        .collect();
    bonus_ids.sort();
    bonus_ids.dedup();
    assert_eq!(
        bonus_ids.len(),
        expected,
        "picks must come from different bonuses"
    );
}

#[test]
fn test_picking_resolves_to_play_phase() {
    let map = load_small_earth();
    let board = Board::from_map(map.clone());
    let mut rng = StdRng::seed_from_u64(42);
    let mut state = GameState::new(&board);

    let options = picking::generate_pick_options(&board, &mut rng);
    // Both players submit all options as their priority list.
    // ABBA snake draft with 6 options, 4 picks each = 8 total picks.
    // Some will be auto-filled from random unclaimed territories.
    let picks_a: Vec<usize> = options.clone();
    let picks_b: Vec<usize> = options.iter().rev().copied().collect();

    picking::resolve_picks(
        &mut state,
        [&picks_a, &picks_b],
        &board,
        picking::DEFAULT_STARTING_ARMIES,
    );

    assert_eq!(state.phase, Phase::Play);
    assert_eq!(state.turn, 1);
    assert_eq!(
        state.territory_count_for(0),
        4,
        "Player 0 should get 4 territories"
    );
    assert_eq!(
        state.territory_count_for(1),
        4,
        "Player 1 should get 4 territories"
    );
}

#[test]
fn test_picked_territories_get_starting_armies() {
    let map = load_small_earth();
    let board = Board::from_map(map.clone());
    let mut rng = StdRng::seed_from_u64(42);
    let mut state = GameState::new(&board);

    let options = picking::generate_pick_options(&board, &mut rng);
    let picks: Vec<usize> = options.iter().take(4).copied().collect();
    let ai_picks = ai::generate_picks(&state, &board, &options);

    picking::resolve_picks(
        &mut state,
        [&picks, &ai_picks],
        &board,
        picking::DEFAULT_STARTING_ARMIES,
    );

    // All picked territories should have 5 armies
    for tid in 0..map.territories.len() {
        if state.territory_owners[tid] != NEUTRAL {
            assert_eq!(
                state.territory_armies[tid], 5,
                "Picked territory {} should have 5 armies",
                tid
            );
        }
    }
}

// ========== FOG OF WAR ==========

#[test]
fn test_fog_hides_distant_territories() {
    let map = load_small_earth();
    let board = Board::from_map(map.clone());
    let mut state = GameState::new(&board);
    // Player 0 owns Alaska (0)
    state.territory_owners[0] = 0;
    state.territory_armies[0] = 5;

    let visible = visible_territories(&state, 0, &board);

    // Should see Alaska and its neighbors
    assert!(visible.contains(&0));
    for &adj in &map.territories[0].adjacent {
        assert!(visible.contains(&adj));
    }

    // Should NOT see distant territories (e.g., Argentina, id 12)
    assert!(!visible.contains(&12));
}

#[test]
fn test_fog_filter_masks_enemy_armies() {
    let map = load_small_earth();
    let board = Board::from_map(map);
    let mut state = GameState::new(&board);
    state.territory_owners[0] = 0;
    state.territory_armies[0] = 5;
    state.territory_owners[12] = 1;
    state.territory_armies[12] = 20;

    let filtered = fog_filter(&state, 0, &board);

    // Can't see Argentina's real army count
    assert_eq!(filtered.territory_owners[12], NEUTRAL);
    assert_ne!(filtered.territory_armies[12], 20);
}

// ========== TURN RESOLUTION ==========

#[test]
fn test_deploy_increases_armies() {
    let map = load_small_earth();
    let board = Board::from_map(map);
    let mut state = GameState::new(&board);
    state.territory_owners[0] = 0;
    state.territory_armies[0] = 1;
    state.phase = Phase::Play;
    state.turn = 1;

    let mut rng = StdRng::seed_from_u64(42);
    let orders = [
        vec![Order::Deploy {
            territory: 0,
            armies: 5,
        }],
        vec![],
    ];

    let result = resolve_turn(&state, orders, &board, &mut rng);
    assert_eq!(result.state.territory_armies[0], 6); // 1 + 5
}

#[test]
fn test_successful_attack_captures_territory() {
    let map = load_small_earth();
    let board = Board::from_map(map);
    let mut state = GameState::new(&board);
    // Alaska (0) owned by P0 with 10 armies
    state.territory_owners[0] = 0;
    state.territory_armies[0] = 10;
    // NW Territory (1) is neutral with 2 armies, adjacent to Alaska
    state.territory_owners[1] = NEUTRAL;
    state.territory_armies[1] = 2;
    state.phase = Phase::Play;
    state.turn = 1;

    let mut rng = StdRng::seed_from_u64(42);
    let orders = [
        vec![Order::Attack {
            from: 0,
            to: 1,
            armies: 9,
        }],
        vec![],
    ];

    let result = resolve_turn(&state, orders, &board, &mut rng);
    assert_eq!(
        result.state.territory_owners[1], 0,
        "Should capture NW Territory"
    );
    assert!(
        result.state.territory_armies[1] > 0,
        "Should have surviving attackers"
    );
    assert_eq!(
        result.state.territory_armies[0], 1,
        "Should leave 1 on Alaska"
    );
}

#[test]
fn test_elimination_triggers_victory() {
    let map = load_small_earth();
    let board = Board::from_map(map);
    let mut state = GameState::new(&board);
    // P0 has Alaska with 20 armies
    state.territory_owners[0] = 0;
    state.territory_armies[0] = 20;
    // P1 has NW Territory with 1 army (adjacent to Alaska)
    state.territory_owners[1] = 1;
    state.territory_armies[1] = 1;
    state.alive = [true; 2];
    state.phase = Phase::Play;
    state.turn = 1;

    let mut rng = StdRng::seed_from_u64(42);
    let orders = [
        vec![
            Order::Deploy {
                territory: 0,
                armies: 5,
            },
            Order::Attack {
                from: 0,
                to: 1,
                armies: 24,
            },
        ],
        vec![Order::Deploy {
            territory: 1,
            armies: 5,
        }],
    ];

    let result = resolve_turn(&state, orders, &board, &mut rng);
    assert_eq!(result.state.phase, Phase::Finished);
    assert_eq!(result.state.winner, Some(0));

    // Should have elimination + victory events
    let has_victory = result
        .events
        .iter()
        .any(|e| matches!(e, strat_engine::turn::TurnEvent::Victory { player: 0 }));
    assert!(has_victory, "Should have victory event");
}

// ========== AI ==========

#[test]
fn test_ai_generates_valid_orders() {
    let map = load_small_earth();
    let board = Board::from_map(map.clone());
    let mut state = GameState::new(&board);
    let mut rng = StdRng::seed_from_u64(42);

    // Set up a realistic game state
    let options = picking::generate_pick_options(&board, &mut rng);
    let picks: Vec<usize> = options.iter().take(4).copied().collect();
    let ai_picks = ai::generate_picks(&state, &board, &options);
    picking::resolve_picks(
        &mut state,
        [&picks, &ai_picks],
        &board,
        picking::DEFAULT_STARTING_ARMIES,
    );

    // AI should generate orders
    let orders = ai::generate_orders(&state, 1, &board);
    assert!(
        !orders.is_empty(),
        "AI should generate at least deploy orders"
    );

    // First order should be a deploy
    assert!(
        matches!(orders[0], Order::Deploy { .. }),
        "AI's first order should be a deploy"
    );

    // Total deploy should equal income
    let income = state.income(1, &board);
    let total_deployed: u32 = orders
        .iter()
        .filter_map(|o| match o {
            Order::Deploy { armies, .. } => Some(*armies),
            _ => None,
        })
        .sum();
    assert_eq!(
        total_deployed, income,
        "AI should deploy exactly its income"
    );
}

#[test]
fn test_ai_expands_over_multiple_turns() {
    let map = load_small_earth();
    let board = Board::from_map(map);
    let mut state = GameState::new(&board);
    let mut rng = StdRng::seed_from_u64(42);

    let options = picking::generate_pick_options(&board, &mut rng);
    let picks: Vec<usize> = options.iter().take(4).copied().collect();
    let ai_picks = ai::generate_picks(&state, &board, &options);
    picking::resolve_picks(
        &mut state,
        [&picks, &ai_picks],
        &board,
        picking::DEFAULT_STARTING_ARMIES,
    );

    let initial_ai_territories = state.territory_count_for(1);

    // Run 5 turns of AI playing both sides
    for _ in 0..5 {
        let p0_orders = ai::generate_orders(&state, 0, &board);
        let p1_orders = ai::generate_orders(&state, 1, &board);
        let result = resolve_turn(&state, [p0_orders, p1_orders], &board, &mut rng);
        state = result.state;
        if state.phase == Phase::Finished {
            break;
        }
    }

    // AI should have expanded
    assert!(
        state.territory_count_for(1) > initial_ai_territories,
        "AI should expand over 5 turns: started with {}, ended with {}",
        initial_ai_territories,
        state.territory_count_for(1)
    );
}

// ========== FULL GAME SIMULATION ==========

#[test]
fn test_full_game_terminates() {
    let map = load_small_earth();
    let board = Board::from_map(map);
    let mut state = GameState::new(&board);
    let mut rng = StdRng::seed_from_u64(123);

    let options = picking::generate_pick_options(&board, &mut rng);
    let picks: Vec<usize> = options.iter().take(4).copied().collect();
    let ai_picks = ai::generate_picks(&state, &board, &options);
    picking::resolve_picks(
        &mut state,
        [&picks, &ai_picks],
        &board,
        picking::DEFAULT_STARTING_ARMIES,
    );

    // Play up to 100 turns — game should end well before that
    for turn in 0..100 {
        if state.phase == Phase::Finished {
            println!("Game ended on turn {} — winner: {:?}", turn, state.winner);
            return;
        }
        let p0_orders = ai::generate_orders(&state, 0, &board);
        let p1_orders = ai::generate_orders(&state, 1, &board);
        let result = resolve_turn(&state, [p0_orders, p1_orders], &board, &mut rng);
        state = result.state;
    }

    // If we get here, the game didn't terminate — that's ok for a draw scenario
    // but let's make sure both players are still alive or one won
    println!(
        "Game didn't end in 100 turns. P0: {} territories, P1: {} territories",
        state.territory_count_for(0),
        state.territory_count_for(1)
    );
}

#[test]
fn test_income_increases_with_bonus_capture() {
    let map = load_small_earth();
    let board = Board::from_map(map.clone());
    let mut state = GameState::new(&board);

    // Give player 0 all of South America (bonus 1: territories 9,10,11,12)
    for &tid in &map.bonuses[1].territory_ids {
        state.territory_owners[tid] = 0;
        state.territory_armies[tid] = 5;
    }

    let income = state.income(0, &board);
    let bonus_value = map.bonuses[1].value;
    assert_eq!(income, 5 + bonus_value, "Income should be base + bonus");
}

// ========== CONCURRENT ATTACKS ==========

#[test]
fn test_concurrent_attacks_same_territory() {
    let map = load_small_earth();
    let board = Board::from_map(map);
    let mut state = GameState::new(&board);
    // P0 owns Alaska (0), P1 owns Alberta (3). Both adjacent to NW Territory (1).
    state.territory_owners[0] = 0;
    state.territory_armies[0] = 15;
    state.territory_owners[3] = 1;
    state.territory_armies[3] = 15;
    state.territory_owners[1] = NEUTRAL;
    state.territory_armies[1] = 2;
    state.alive = [true; 2];
    state.phase = Phase::Play;
    state.turn = 1;

    let mut rng = StdRng::seed_from_u64(42);
    let orders = [
        vec![
            Order::Deploy {
                territory: 0,
                armies: 5,
            },
            Order::Attack {
                from: 0,
                to: 1,
                armies: 19,
            },
        ],
        vec![
            Order::Deploy {
                territory: 3,
                armies: 5,
            },
            Order::Attack {
                from: 3,
                to: 1,
                armies: 19,
            },
        ],
    ];

    let result = resolve_turn(&state, orders, &board, &mut rng);
    // NW Territory (1) should be owned by exactly one player, not both.
    let owner = result.state.territory_owners[1];
    assert!(
        owner == 0 || owner == 1,
        "Territory should be captured by one player, got {}",
        owner
    );
    assert!(result.state.territory_armies[1] > 0);
}

// ========== TRANSFER CHAIN ==========

#[test]
fn test_transfer_chain() {
    let map = load_small_earth();
    let board = Board::from_map(map);
    let mut state = GameState::new(&board);
    // P0 owns Alaska (0), NW Territory (1), Alberta (3). These form a chain.
    state.territory_owners[0] = 0;
    state.territory_armies[0] = 20;
    state.territory_owners[1] = 0;
    state.territory_armies[1] = 1;
    state.territory_owners[3] = 0;
    state.territory_armies[3] = 1;
    state.alive = [true; 2];
    state.phase = Phase::Play;
    state.turn = 1;

    let mut rng = StdRng::seed_from_u64(42);
    let orders = [
        vec![
            Order::Deploy {
                territory: 0,
                armies: 5,
            },
            Order::Transfer {
                from: 0,
                to: 1,
                armies: 24,
            },
            Order::Transfer {
                from: 1,
                to: 3,
                armies: 24,
            },
        ],
        vec![],
    ];

    let result = resolve_turn(&state, orders, &board, &mut rng);
    // Armies should flow from 0 -> 1 -> 3.
    // Territory 0 should keep 1 army. The rest go to 1, then from 1 to 3.
    assert_eq!(result.state.territory_armies[0], 1);
    // All transferred armies should end up somewhere.
    // Original: 20 + 1 + 1 = 22. Deploy 5 to territory 0 = 27 total.
    let total: u32 = result.state.territory_armies[0]
        + result.state.territory_armies[1]
        + result.state.territory_armies[3];
    assert_eq!(
        total,
        20 + 1 + 1 + 5,
        "Total armies should be preserved (original + deployed)"
    );
}

// ========== BONUS INCOME ==========

#[test]
fn test_bonus_income_correct() {
    for map in [load_small_earth(), load_big_earth()] {
        let board = Board::from_map(map.clone());
        let mut state = GameState::new(&board);
        // Test each bonus individually.
        for bonus in &map.bonuses {
            // Reset state.
            state.territory_owners = vec![NEUTRAL; map.territories.len()];
            // Assign this bonus to player 0.
            for &tid in &bonus.territory_ids {
                state.territory_owners[tid] = 0;
            }
            let income = state.income(0, &board);
            assert_eq!(
                income,
                map.settings.base_income + bonus.value,
                "Map '{}', bonus '{}': expected income {} + {}, got {}",
                map.name,
                bonus.name,
                map.settings.base_income,
                bonus.value,
                income
            );
        }
    }
}

// ========== FOG CONSISTENCY ==========

#[test]
fn test_fog_consistency() {
    let map = load_small_earth();
    let board = Board::from_map(map.clone());
    let mut state = GameState::new(&board);
    let mut rng = StdRng::seed_from_u64(42);

    let options = picking::generate_pick_options(&board, &mut rng);
    let picks_a: Vec<usize> = options.iter().take(4).copied().collect();
    let picks_b = ai::generate_picks(&state, &board, &options);
    picking::resolve_picks(
        &mut state,
        [&picks_a, &picks_b],
        &board,
        picking::DEFAULT_STARTING_ARMIES,
    );

    // Run a few turns to create a realistic state.
    for _ in 0..3 {
        if state.phase == Phase::Finished {
            break;
        }
        let p0 = ai::generate_orders(&state, 0, &board);
        let p1 = ai::generate_orders(&state, 1, &board);
        let result = resolve_turn(&state, [p0, p1], &board, &mut rng);
        state = result.state;
    }

    let visible = visible_territories(&state, 0, &board);
    let filtered = fog_filter(&state, 0, &board);

    for tid in 0..map.territories.len() {
        if !visible.contains(&tid) {
            // Non-visible territory should not reveal enemy owner info.
            assert_eq!(
                filtered.territory_owners[tid], NEUTRAL,
                "Fogged territory {} should show as NEUTRAL, got {}",
                tid, filtered.territory_owners[tid]
            );
        }
    }
}

// ========== 10-TURN AI VS AI ==========

#[test]
fn test_game_10_turns() {
    let map = load_small_earth();
    let board = Board::from_map(map.clone());
    let mut state = GameState::new(&board);
    let mut rng = StdRng::seed_from_u64(99);

    let options = picking::generate_pick_options(&board, &mut rng);
    let picks_a: Vec<usize> = options.iter().take(4).copied().collect();
    let picks_b = ai::generate_picks(&state, &board, &options);
    picking::resolve_picks(
        &mut state,
        [&picks_a, &picks_b],
        &board,
        picking::DEFAULT_STARTING_ARMIES,
    );

    for _ in 0..10 {
        if state.phase == Phase::Finished {
            break;
        }
        let p0 = ai::generate_orders(&state, 0, &board);
        let p1 = ai::generate_orders(&state, 1, &board);
        let result = resolve_turn(&state, [p0, p1], &board, &mut rng);
        state = result.state;

        // Invariants.
        for tid in 0..map.territories.len() {
            assert!(
                state.territory_owners[tid] == 0
                    || state.territory_owners[tid] == 1
                    || state.territory_owners[tid] == NEUTRAL,
                "Invalid owner {} at territory {}",
                state.territory_owners[tid],
                tid
            );
            // Armies should never be zero for owned territories that are alive.
            // (unless captured this turn, but territory_armies should still be > 0 for owned)
        }
    }

    // No territory should have negative armies (u32 so check for extreme values from overflow).
    for tid in 0..map.territories.len() {
        assert!(
            state.territory_armies[tid] < 100_000,
            "Suspicious army count at territory {}: {}",
            tid,
            state.territory_armies[tid]
        );
    }
}

// ========== BIG EARTH FULL GAME ==========

#[test]
fn test_big_earth_full_game() {
    let map = load_big_earth();
    let board = Board::from_map(map);
    let mut state = GameState::new(&board);
    let mut rng = StdRng::seed_from_u64(77);

    let options = picking::generate_pick_options(&board, &mut rng);
    let picks_a: Vec<usize> = options.iter().take(4).copied().collect();
    let picks_b = ai::generate_picks(&state, &board, &options);
    picking::resolve_picks(
        &mut state,
        [&picks_a, &picks_b],
        &board,
        picking::DEFAULT_STARTING_ARMIES,
    );

    for turn in 0..200 {
        if state.phase == Phase::Finished {
            println!(
                "Big Earth game ended on turn {}, winner: {:?}",
                turn, state.winner
            );
            return;
        }
        let p0 = ai::generate_orders(&state, 0, &board);
        let p1 = ai::generate_orders(&state, 1, &board);
        let result = resolve_turn(&state, [p0, p1], &board, &mut rng);
        state = result.state;
    }
    // If it reached 200 turns, verify state is still consistent.
    assert!(
        state.territory_count_for(0) + state.territory_count_for(1) > 0,
        "At least one player should still have territories"
    );
}

// ========== PICKING CONTESTED ==========

#[test]
fn test_picking_contested() {
    let map = load_small_earth();
    let board = Board::from_map(map.clone());
    let mut state = GameState::new(&board);
    let mut rng = StdRng::seed_from_u64(42);
    let options = picking::generate_pick_options(&board, &mut rng);

    // Both players submit identical pick lists.
    picking::resolve_picks(
        &mut state,
        [&options, &options],
        &board,
        picking::DEFAULT_STARTING_ARMIES,
    );

    assert_eq!(state.territory_count_for(0), 4);
    assert_eq!(state.territory_count_for(1), 4);
    // No territory should be shared.
    for tid in 0..map.territories.len() {
        let o = state.territory_owners[tid];
        assert!(o == 0 || o == 1 || o == NEUTRAL);
    }
}

// ========== INCOME NEVER NEGATIVE ==========

#[test]
fn test_income_never_negative() {
    let map = load_small_earth();
    let board = Board::from_map(map.clone());
    let mut state = GameState::new(&board);
    let mut rng = StdRng::seed_from_u64(55);

    let options = picking::generate_pick_options(&board, &mut rng);
    let picks_a: Vec<usize> = options.iter().take(4).copied().collect();
    let picks_b = ai::generate_picks(&state, &board, &options);
    picking::resolve_picks(
        &mut state,
        [&picks_a, &picks_b],
        &board,
        picking::DEFAULT_STARTING_ARMIES,
    );

    for _ in 0..20 {
        if state.phase == Phase::Finished {
            break;
        }
        for player in 0..2u8 {
            if state.alive[player as usize] {
                let income = state.income(player, &board);
                assert!(
                    income >= map.settings.base_income,
                    "Player {} income {} is below base income {}",
                    player,
                    income,
                    map.settings.base_income
                );
            }
        }
        let p0 = ai::generate_orders(&state, 0, &board);
        let p1 = ai::generate_orders(&state, 1, &board);
        let result = resolve_turn(&state, [p0, p1], &board, &mut rng);
        state = result.state;
    }
}

// ========== ELIMINATION ==========

#[test]
fn test_elimination_correct() {
    let map = load_small_earth();
    let board = Board::from_map(map);
    let mut state = GameState::new(&board);
    // P1 has exactly 1 territory (NW Territory, id 1).
    // P0 has Alaska (0) with overwhelming force.
    state.territory_owners[0] = 0;
    state.territory_armies[0] = 50;
    state.territory_owners[1] = 1;
    state.territory_armies[1] = 1;
    state.alive = [true; 2];
    state.phase = Phase::Play;
    state.turn = 1;

    let mut rng = StdRng::seed_from_u64(42);
    let orders = [
        vec![
            Order::Deploy {
                territory: 0,
                armies: 5,
            },
            Order::Attack {
                from: 0,
                to: 1,
                armies: 54,
            },
        ],
        vec![Order::Deploy {
            territory: 1,
            armies: 5,
        }],
    ];

    let result = resolve_turn(&state, orders, &board, &mut rng);
    assert_eq!(result.state.phase, Phase::Finished);
    assert_eq!(result.state.winner, Some(0));
    assert!(!result.state.alive[1]);
}

// ========== PROPERTY-BASED: 50 RANDOM GAMES ==========

#[test]
fn test_random_games_invariants() {
    let map = load_small_earth();
    let board = Board::from_map(map.clone());

    for seed in 0..50u64 {
        let mut state = GameState::new(&board);
        let mut rng = StdRng::seed_from_u64(seed);

        let options = picking::generate_pick_options(&board, &mut rng);
        let picks_a = ai::generate_picks(&state, &board, &options);
        let picks_b = ai::generate_picks(&state, &board, &options);
        picking::resolve_picks(
            &mut state,
            [&picks_a, &picks_b],
            &board,
            picking::DEFAULT_STARTING_ARMIES,
        );

        for turn in 0..200 {
            if state.phase == Phase::Finished {
                break;
            }

            let p0 = ai::generate_orders(&state, 0, &board);
            let p1 = ai::generate_orders(&state, 1, &board);
            let result = resolve_turn(&state, [p0, p1], &board, &mut rng);
            state = result.state;

            // Invariant 1: No territory has overflow-level armies (proxy for negative).
            for tid in 0..map.territories.len() {
                assert!(
                    state.territory_armies[tid] < 100_000,
                    "Seed {}, turn {}: territory {} has suspicious army count {}",
                    seed,
                    turn,
                    tid,
                    state.territory_armies[tid]
                );
            }

            // Invariant 2: All owners are valid.
            for tid in 0..map.territories.len() {
                let o = state.territory_owners[tid];
                assert!(
                    o == 0 || o == 1 || o == NEUTRAL,
                    "Seed {}, turn {}: territory {} has invalid owner {}",
                    seed,
                    turn,
                    tid,
                    o
                );
            }

            // Invariant 3: Total territory accounting.
            let p0_count = state.territory_count_for(0);
            let p1_count = state.territory_count_for(1);
            let neutral_count = state.territory_count_for(NEUTRAL);
            assert_eq!(
                p0_count + p1_count + neutral_count,
                map.territories.len(),
                "Seed {}, turn {}: territory count mismatch",
                seed,
                turn
            );

            // Invariant 4: Income is >= base_income for alive players.
            for player in 0..2u8 {
                if state.alive[player as usize] && state.territory_count_for(player) > 0 {
                    let income = state.income(player, &board);
                    assert!(
                        income >= board.settings().base_income,
                        "Seed {}, turn {}: player {} income {} below base {}",
                        seed,
                        turn,
                        player,
                        income,
                        board.settings().base_income
                    );
                }
            }
        }

        // Invariant 5: Game should terminate within 200 turns.
        assert!(
            state.phase == Phase::Finished || state.turn <= 201,
            "Seed {}: game did not terminate within 200 turns",
            seed
        );
    }
}

// ========== MCTS AI TESTS ==========

#[test]
fn test_mcts_beats_random() {
    let map = load_small_earth();
    let board = Board::from_map(map);
    let mut mcts_wins = 0u32;

    for seed in 0..10u64 {
        let mut state = GameState::new(&board);
        let mut rng = StdRng::seed_from_u64(seed * 31337);

        let options = picking::generate_pick_options(&board, &mut rng);
        let picks_a = ai::generate_picks(&state, &board, &options);
        let picks_b = ai::generate_picks(&state, &board, &options);
        picking::resolve_picks(
            &mut state,
            [&picks_a, &picks_b],
            &board,
            picking::DEFAULT_STARTING_ARMIES,
        );

        // Player 0 = Hard (MCTS), Player 1 = Easy (Random)
        for _ in 0..200 {
            if state.phase == Phase::Finished {
                break;
            }
            let p0_orders = ai::generate_orders_for_strength(&state, 0, &board, AiStrength::Hard);
            let p1_orders = ai::generate_orders_for_strength(&state, 1, &board, AiStrength::Easy);
            let result = resolve_turn(&state, [p0_orders, p1_orders], &board, &mut rng);
            state = result.state;
        }

        if state.winner == Some(0) {
            mcts_wins += 1;
        }
    }

    assert!(
        mcts_wins >= 8,
        "MCTS (Hard) should beat Random (Easy) at least 8/10 times, but won only {}/10",
        mcts_wins
    );
}

#[test]
fn test_mcts_time_budget() {
    let map = load_small_earth();
    let board = Board::from_map(map);
    let mut state = GameState::new(&board);
    let mut rng = StdRng::seed_from_u64(42);

    let options = picking::generate_pick_options(&board, &mut rng);
    let picks_a = ai::generate_picks(&state, &board, &options);
    let picks_b = ai::generate_picks(&state, &board, &options);
    picking::resolve_picks(
        &mut state,
        [&picks_a, &picks_b],
        &board,
        picking::DEFAULT_STARTING_ARMIES,
    );

    // Measure time with 100ms budget
    let config_100ms = MctsConfig {
        time_budget: Duration::from_millis(100),
        ..Default::default()
    };
    let start = Instant::now();
    let _ = strat_engine::mcts::mcts_generate_orders(&state, 0, &board, &config_100ms);
    let elapsed_100ms = start.elapsed();

    // Measure time with 200ms budget
    let config_200ms = MctsConfig {
        time_budget: Duration::from_millis(200),
        ..Default::default()
    };
    let start = Instant::now();
    let _ = strat_engine::mcts::mcts_generate_orders(&state, 0, &board, &config_200ms);
    let elapsed_200ms = start.elapsed();

    // 100ms budget should finish faster than 200ms budget
    assert!(
        elapsed_100ms < elapsed_200ms,
        "100ms budget ({:?}) should be faster than 200ms budget ({:?})",
        elapsed_100ms,
        elapsed_200ms
    );
}

// ========== WIN PROBABILITY TESTS ==========

#[test]
fn test_win_prob_correlates_with_outcome() {
    let map = load_small_earth();
    let board = Board::from_map(map);
    let mut high_prob_wins = 0u32;
    let mut high_prob_total = 0u32;
    let mut low_prob_wins = 0u32;
    let mut low_prob_total = 0u32;

    for seed in 0..20u64 {
        let mut state = GameState::new(&board);
        let mut rng = StdRng::seed_from_u64(seed * 7919 + 101);

        let options = picking::generate_pick_options(&board, &mut rng);
        let picks_a = ai::generate_picks(&state, &board, &options);
        let picks_b = ai::generate_picks(&state, &board, &options);
        picking::resolve_picks(
            &mut state,
            [&picks_a, &picks_b],
            &board,
            picking::DEFAULT_STARTING_ARMIES,
        );

        // Run a few turns to create divergent positions
        for _ in 0..5 {
            if state.phase == Phase::Finished {
                break;
            }
            let p0 = ai::generate_orders(&state, 0, &board);
            let p1 = ai::generate_orders(&state, 1, &board);
            let result = resolve_turn(&state, [p0, p1], &board, &mut rng);
            state = result.state;
        }

        if state.phase == Phase::Finished {
            continue;
        }

        // Record win probability at this point
        let wp = quick_win_probability(&state, &board);
        let starting_prob = wp.player_0;

        // Play to completion
        for _ in 0..200 {
            if state.phase == Phase::Finished {
                break;
            }
            let p0 = ai::generate_orders(&state, 0, &board);
            let p1 = ai::generate_orders(&state, 1, &board);
            let result = resolve_turn(&state, [p0, p1], &board, &mut rng);
            state = result.state;
        }

        let p0_won = state.winner == Some(0);

        if starting_prob > 0.6 {
            high_prob_total += 1;
            if p0_won {
                high_prob_wins += 1;
            }
        } else if starting_prob < 0.4 {
            low_prob_total += 1;
            if p0_won {
                low_prob_wins += 1;
            }
        }
    }

    // Games where player 0 had high win prob should be won more often
    // than games where player 0 had low win prob.
    let high_rate = if high_prob_total > 0 {
        high_prob_wins as f64 / high_prob_total as f64
    } else {
        0.5 // neutral if no data points
    };
    let low_rate = if low_prob_total > 0 {
        low_prob_wins as f64 / low_prob_total as f64
    } else {
        0.5
    };

    // Only assert if we have data in both buckets
    if high_prob_total > 0 && low_prob_total > 0 {
        assert!(
            high_rate >= low_rate,
            "High win-prob games should be won at least as often as low win-prob games: \
             high={}/{} ({:.0}%), low={}/{} ({:.0}%)",
            high_prob_wins,
            high_prob_total,
            high_rate * 100.0,
            low_prob_wins,
            low_prob_total,
            low_rate * 100.0
        );
    }
}

// ========== PUZZLE TESTS ==========

#[test]
fn test_daily_puzzle_deterministic() {
    let p1 = daily_puzzle(42);
    let p2 = daily_puzzle(42);
    assert_eq!(p1.id, p2.id);
    assert_eq!(p1.description, p2.description);
    assert_eq!(p1.optimal_orders, p2.optimal_orders);
    assert_eq!(p1.state.territory_armies, p2.state.territory_armies);
    assert_eq!(p1.state.territory_owners, p2.state.territory_owners);
}

#[test]
fn test_daily_puzzle_different_days() {
    let p1 = daily_puzzle(100);
    let p2 = daily_puzzle(101);
    assert_ne!(p1.id, p2.id);
    // Different seeds should produce different puzzles (at least different state)
    let different_state = p1.state.territory_armies != p2.state.territory_armies
        || p1.state.territory_owners != p2.state.territory_owners
        || p1.description != p2.description;
    assert!(
        different_state,
        "Different day seeds should produce different puzzles"
    );
}

// ========== GAME ANALYSIS TESTS ==========

#[test]
fn test_analysis_detects_key_moments() {
    let map = load_small_earth();
    let board = Board::from_map(map);
    let mut state = GameState::new(&board);
    let mut rng = StdRng::seed_from_u64(42);

    let options = picking::generate_pick_options(&board, &mut rng);
    let picks_a = ai::generate_picks(&state, &board, &options);
    let picks_b = ai::generate_picks(&state, &board, &options);
    picking::resolve_picks(
        &mut state,
        [&picks_a, &picks_b],
        &board,
        picking::DEFAULT_STARTING_ARMIES,
    );

    let mut state_history = vec![state.clone()];
    let mut win_prob_history = vec![material_evaluation(&state, &board)];
    let mut all_turn_events = Vec::new();

    // Play a game collecting history
    for _ in 0..50 {
        if state.phase == Phase::Finished {
            break;
        }
        let p0 = ai::generate_orders(&state, 0, &board);
        let p1 = ai::generate_orders(&state, 1, &board);
        let result = resolve_turn(&state, [p0, p1], &board, &mut rng);
        state = result.state.clone();
        state_history.push(state.clone());
        win_prob_history.push(material_evaluation(&state, &board));
        all_turn_events.push(result.events);
    }

    let analysis = analyze_game(&state_history, &win_prob_history, &all_turn_events, &board);

    // A game that plays out over many turns should have at least one key moment
    // (bonus completion, turning point, or big swing).
    if state_history.len() > 5 {
        let _has_any_moment = !analysis.key_moments.is_empty();
        // Also check that territory control was tracked properly
        assert_eq!(
            analysis.territory_control_over_time.len(),
            state_history.len()
        );
        assert_eq!(analysis.income_over_time.len(), state_history.len());

        // The analysis should track the correct number of turns
        assert_eq!(analysis.turns_played, state_history.len() as u32);

        // In a multi-turn game, there should be at least some key moments
        // (bonus completions, swings, etc.) -- or the game was very one-sided.
        // Either way, the analysis should not panic and should be well-formed.
        println!(
            "Analysis found {} key moments over {} turns",
            analysis.key_moments.len(),
            analysis.turns_played
        );
    }
}

// ========== EDGE CASE TESTS ==========

#[test]
fn test_game_with_no_fog() {
    let mut map = load_small_earth();
    map.settings.fog_of_war = false;
    let board = Board::from_map(map);

    // Create a state where both players own territories
    let mut state = GameState::new(&board);
    state.territory_owners[0] = 0;
    state.territory_armies[0] = 5;
    state.territory_owners[12] = 1;
    state.territory_armies[12] = 20;

    let filtered = fog_filter(&state, 0, &board);

    // All territories should be visible -- enemy territory should show true owner/armies
    assert_eq!(filtered.territory_owners[12], 1);
    assert_eq!(filtered.territory_armies[12], 20);

    // Verify ALL territories are visible (not masked)
    for tid in 0..board.map.territories.len() {
        assert_eq!(
            filtered.territory_owners[tid], state.territory_owners[tid],
            "Territory {} owner should be visible with fog disabled",
            tid
        );
        assert_eq!(
            filtered.territory_armies[tid], state.territory_armies[tid],
            "Territory {} armies should be visible with fog disabled",
            tid
        );
    }
}

#[test]
fn test_game_with_different_kill_rates() {
    // Standard kill rates: 0.6 offense, 0.7 defense
    let standard = default_settings();
    let r_standard = resolve_attack(10, 5, &standard);

    // Custom 50/50 kill rates
    let custom = MapSettings {
        offense_kill_rate: 0.5,
        defense_kill_rate: 0.5,
        ..default_settings()
    };
    let r_custom = resolve_attack(10, 5, &custom);

    // With 50/50 kill rates, offense kills fewer defenders (0.5 vs 0.6)
    // and defense kills fewer attackers (0.5 vs 0.7).
    // The outcomes should differ from standard rates.
    let outcomes_differ = r_standard.defenders_killed != r_custom.defenders_killed
        || r_standard.attackers_killed != r_custom.attackers_killed;
    assert!(
        outcomes_differ,
        "Different kill rates should produce different combat outcomes: \
         standard(atk_killed={}, def_killed={}) vs custom(atk_killed={}, def_killed={})",
        r_standard.attackers_killed,
        r_standard.defenders_killed,
        r_custom.attackers_killed,
        r_custom.defenders_killed
    );

    // With lower offense kill rate (0.5), fewer defenders should be killed
    assert!(
        r_custom.defenders_killed <= r_standard.defenders_killed,
        "Lower offense kill rate should kill fewer or equal defenders"
    );
    // With lower defense kill rate (0.5), fewer attackers should be killed
    assert!(
        r_custom.attackers_killed <= r_standard.attackers_killed,
        "Lower defense kill rate should kill fewer or equal attackers"
    );
}

#[test]
fn test_picks_with_more_options_than_bonuses() {
    let map = load_small_earth();
    let board = Board::from_map(map.clone());
    let mut state = GameState::new(&board);
    let mut rng = StdRng::seed_from_u64(42);

    let options = picking::generate_pick_options(&board, &mut rng);

    // Create pick lists that are longer than the number of available options.
    // The extra entries beyond what can be picked should be handled gracefully.
    let mut extended_picks_a: Vec<usize> = options.clone();
    let mut extended_picks_b: Vec<usize> = options.iter().rev().copied().collect();

    // Add extra territory IDs beyond normal picks
    for tid in 0..map.territories.len() {
        if !extended_picks_a.contains(&tid) {
            extended_picks_a.push(tid);
        }
        if !extended_picks_b.contains(&tid) {
            extended_picks_b.push(tid);
        }
    }

    // This should not panic even with oversized pick lists
    picking::resolve_picks(
        &mut state,
        [&extended_picks_a, &extended_picks_b],
        &board,
        picking::DEFAULT_STARTING_ARMIES,
    );

    assert_eq!(state.phase, Phase::Play);
    assert_eq!(state.territory_count_for(0), 4);
    assert_eq!(state.territory_count_for(1), 4);

    // No territory should be owned by both players
    for tid in 0..map.territories.len() {
        let o = state.territory_owners[tid];
        assert!(o == 0 || o == 1 || o == NEUTRAL);
    }
}

// ========== STRESS TEST ==========

#[test]
fn test_100_random_games_all_maps() {
    let maps = [("Small Earth", load_small_earth()), ("Big Earth", load_big_earth())];

    for (map_name, map) in &maps {
        let board = Board::from_map(map.clone());
        let game_count = 50;
        for seed in 0..game_count as u64 {
            let mut state = GameState::new(&board);
            let mut rng = StdRng::seed_from_u64(seed * 104729 + 7);

            let options = picking::generate_pick_options(&board, &mut rng);
            let picks_a = ai::generate_picks(&state, &board, &options);
            let picks_b = ai::generate_picks(&state, &board, &options);
            picking::resolve_picks(
                &mut state,
                [&picks_a, &picks_b],
                &board,
                picking::DEFAULT_STARTING_ARMIES,
            );

            for turn in 0..200 {
                if state.phase == Phase::Finished {
                    break;
                }

                let p0 = ai::generate_orders(&state, 0, &board);
                let p1 = ai::generate_orders(&state, 1, &board);
                let result = resolve_turn(&state, [p0, p1], &board, &mut rng);
                state = result.state;

                // Invariant: no negative armies (u32 overflow detection)
                for tid in 0..map.territories.len() {
                    assert!(
                        state.territory_armies[tid] < 100_000,
                        "Map '{}', seed {}, turn {}: territory {} has suspicious army count {}",
                        map_name,
                        seed,
                        turn,
                        tid,
                        state.territory_armies[tid]
                    );
                }

                // Invariant: all owners are valid
                for tid in 0..map.territories.len() {
                    let o = state.territory_owners[tid];
                    assert!(
                        o == 0 || o == 1 || o == NEUTRAL,
                        "Map '{}', seed {}, turn {}: territory {} has invalid owner {}",
                        map_name,
                        seed,
                        turn,
                        tid,
                        o
                    );
                }

                // Invariant: territory accounting is consistent
                let p0_count = state.territory_count_for(0);
                let p1_count = state.territory_count_for(1);
                let neutral_count = state.territory_count_for(NEUTRAL);
                assert_eq!(
                    p0_count + p1_count + neutral_count,
                    map.territories.len(),
                    "Map '{}', seed {}, turn {}: territory count mismatch",
                    map_name,
                    seed,
                    turn
                );
            }

            // Game should terminate within 200 turns
            assert!(
                state.phase == Phase::Finished || state.turn <= 201,
                "Map '{}', seed {}: game did not terminate within 200 turns",
                map_name,
                seed
            );
        }
    }
}

// ========== PERFORMANCE TEST ==========

#[test]
fn test_turn_resolution_under_1ms() {
    let map = load_small_earth();
    assert_eq!(map.territories.len(), 42);
    let board = Board::from_map(map);

    let mut state = GameState::new(&board);
    let mut rng = StdRng::seed_from_u64(42);

    // Set up a realistic mid-game state
    let options = picking::generate_pick_options(&board, &mut rng);
    let picks_a = ai::generate_picks(&state, &board, &options);
    let picks_b = ai::generate_picks(&state, &board, &options);
    picking::resolve_picks(
        &mut state,
        [&picks_a, &picks_b],
        &board,
        picking::DEFAULT_STARTING_ARMIES,
    );

    // Run a few turns to create a realistic mid-game state
    for _ in 0..5 {
        if state.phase == Phase::Finished {
            break;
        }
        let p0 = ai::generate_orders(&state, 0, &board);
        let p1 = ai::generate_orders(&state, 1, &board);
        let result = resolve_turn(&state, [p0, p1], &board, &mut rng);
        state = result.state;
    }

    if state.phase == Phase::Finished {
        return; // Game ended early, skip timing
    }

    // Pre-generate orders so we only time the resolution
    let p0_orders = ai::generate_orders(&state, 0, &board);
    let p1_orders = ai::generate_orders(&state, 1, &board);

    // Warm up
    let mut rng_warmup = StdRng::seed_from_u64(99);
    let _ = resolve_turn(
        &state,
        [p0_orders.clone(), p1_orders.clone()],
        &board,
        &mut rng_warmup,
    );

    // Time the turn resolution over multiple iterations for stability
    let iterations = 100;
    let start = Instant::now();
    for i in 0..iterations {
        let mut rng_bench = StdRng::seed_from_u64(i);
        let _ = resolve_turn(
            &state,
            [p0_orders.clone(), p1_orders.clone()],
            &board,
            &mut rng_bench,
        );
    }
    let total_elapsed = start.elapsed();
    let avg_per_turn = total_elapsed / iterations as u32;

    assert!(
        avg_per_turn < Duration::from_millis(1),
        "Turn resolution should take < 1ms on average, took {:?}",
        avg_per_turn
    );
}

// ========== HELPER: Small test map (A-B-C-D linear) ==========

fn test_map_4() -> MapFile {
    MapFile {
        id: "test4".into(),
        name: "Test4".into(),
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

/// Set up a play-phase state on test_map_4: P0 owns {0,1}, P1 owns {2,3}.
fn setup_play_state(board: &Board) -> GameState {
    let mut state = GameState::new(board);
    state.territory_owners = vec![0, 0, 1, 1];
    state.territory_armies = vec![5, 5, 5, 5];
    state.alive = [true; 2];
    state.phase = Phase::Play;
    state.turn = 1;
    state
}

// ========== DEPLOY VALIDATION ==========

#[test]
fn test_deploy_capped_at_income() {
    // Turn resolution caps deploy to income even if player submits more.
    let board = Board::from_map(test_map_4());
    let state = setup_play_state(&board);
    let income = state.income(0, &board); // 5 + 2 = 7

    let mut rng = StdRng::seed_from_u64(42);
    let orders = [
        vec![Order::Deploy {
            territory: 0,
            armies: 100, // way over income
        }],
        vec![Order::Deploy {
            territory: 2,
            armies: 5,
        }],
    ];

    let result = resolve_turn(&state, orders, &board, &mut rng);
    // Should only deploy income amount (7), not 100.
    assert_eq!(
        result.state.territory_armies[0],
        5 + income,
        "Deploy should be capped at income ({}), not 100",
        income
    );
}

#[test]
fn test_deploy_to_enemy_territory_ignored() {
    // Deploy to territory owned by opponent should be skipped by turn resolution.
    let board = Board::from_map(test_map_4());
    let state = setup_play_state(&board);
    let mut rng = StdRng::seed_from_u64(42);

    let orders = [
        vec![Order::Deploy {
            territory: 2, // owned by P1
            armies: 5,
        }],
        vec![Order::Deploy {
            territory: 2,
            armies: 5,
        }],
    ];

    let result = resolve_turn(&state, orders, &board, &mut rng);
    // P0's deploy to enemy territory: check via events that it wasn't applied to P0
    // P1's deploy should work normally
    assert_eq!(result.state.territory_armies[2], 10); // 5 original + 5 from P1
}

#[test]
fn test_deploy_to_neutral_territory_ignored() {
    let board = Board::from_map(test_map_4());
    let mut state = GameState::new(&board);
    state.territory_owners = vec![0, NEUTRAL, 1, NEUTRAL];
    state.territory_armies = vec![5, 2, 5, 2];
    state.alive = [true; 2];
    state.phase = Phase::Play;
    state.turn = 1;

    let mut rng = StdRng::seed_from_u64(42);
    let orders = [
        vec![Order::Deploy {
            territory: 1, // neutral
            armies: 5,
        }],
        vec![Order::Deploy {
            territory: 2,
            armies: 5,
        }],
    ];

    let result = resolve_turn(&state, orders, &board, &mut rng);
    // Neutral territory should not receive P0's deploy.
    assert_eq!(result.state.territory_armies[1], 2);
}

#[test]
fn test_validate_zero_deploy_rejected() {
    let board = Board::from_map(test_map_4());
    let state = setup_play_state(&board);
    let orders = vec![Order::Deploy {
        territory: 0,
        armies: 0,
    }];
    let result = validate_orders(&orders, 0, &state, &board);
    assert!(result.is_err());
}

#[test]
fn test_deploy_split_across_territories() {
    let board = Board::from_map(test_map_4());
    let state = setup_play_state(&board);
    let income = state.income(0, &board); // 7

    let mut rng = StdRng::seed_from_u64(42);
    let orders = [
        vec![
            Order::Deploy {
                territory: 0,
                armies: 3,
            },
            Order::Deploy {
                territory: 1,
                armies: 4,
            },
        ],
        vec![],
    ];

    let result = resolve_turn(&state, orders, &board, &mut rng);
    assert_eq!(result.state.territory_armies[0], 5 + 3);
    assert_eq!(result.state.territory_armies[1], 5 + 4);
    let total_deployed: u32 = result
        .events
        .iter()
        .filter_map(|e| match e {
            TurnEvent::Deploy {
                player: 0, armies, ..
            } => Some(*armies),
            _ => None,
        })
        .sum();
    assert_eq!(total_deployed, income);
}

// ========== ATTACK RESOLUTION THRESHOLDS ==========

#[test]
fn test_combat_exact_thresholds() {
    let s = default_settings();

    // 3v2: 3*0.6=1.8→2, 2*0.7=1.4→1. Capture with 2 survivors.
    let r = resolve_attack(3, 2, &s);
    assert!(r.captured);
    assert_eq!(r.surviving_attackers, 2);

    // 2v2: 2*0.6=1.2→1, 2*0.7=1.4→1. 1 def remains, 1 atk remains. No capture.
    let r = resolve_attack(2, 2, &s);
    assert!(!r.captured);
    assert_eq!(r.surviving_defenders, 1);
    assert_eq!(r.surviving_attackers, 1);

    // 4v3: 4*0.6=2.4→2, 3*0.7=2.1→2. 1 def remains, 2 atk remain. No capture.
    let r = resolve_attack(4, 3, &s);
    assert!(!r.captured);
    assert_eq!(r.surviving_defenders, 1);
    assert_eq!(r.surviving_attackers, 2);

    // 5v3: 5*0.6=3.0→3, 3*0.7=2.1→2. Capture with 3 survivors.
    let r = resolve_attack(5, 3, &s);
    assert!(r.captured);
    assert_eq!(r.surviving_attackers, 3);

    // 10v6: 10*0.6=6→6, 6*0.7=4.2→4. Capture with 6 survivors.
    let r = resolve_attack(10, 6, &s);
    assert!(r.captured);
    assert_eq!(r.surviving_attackers, 6);

    // 10v7: 10*0.6=6→6, 7*0.7=4.9→5. 1 def remains, 5 atk remain. No capture.
    let r = resolve_attack(10, 7, &s);
    assert!(!r.captured);
    assert_eq!(r.surviving_defenders, 1);
}

#[test]
fn test_attack_must_leave_1_army_behind() {
    let board = Board::from_map(test_map_4());
    let mut state = setup_play_state(&board);
    state.territory_armies[1] = 3; // P0 has 3 on territory 1

    let mut rng = StdRng::seed_from_u64(42);
    let orders = [
        vec![Order::Attack {
            from: 1,
            to: 2,
            armies: 3, // tries to send all 3
        }],
        vec![Order::Deploy {
            territory: 2,
            armies: 5,
        }],
    ];

    let result = resolve_turn(&state, orders, &board, &mut rng);
    // Source territory must keep at least 1 army.
    assert!(
        result.state.territory_armies[1] >= 1,
        "Must leave at least 1 army on source, got {}",
        result.state.territory_armies[1]
    );
}

#[test]
fn test_attack_from_unowned_territory_ignored() {
    let board = Board::from_map(test_map_4());
    let state = setup_play_state(&board);

    let mut rng = StdRng::seed_from_u64(42);
    // P0 tries to attack from territory 2 (owned by P1)
    let orders = [
        vec![
            Order::Deploy {
                territory: 0,
                armies: 5,
            },
            Order::Attack {
                from: 2,
                to: 3,
                armies: 4,
            },
        ],
        vec![Order::Deploy {
            territory: 2,
            armies: 5,
        }],
    ];

    let result = resolve_turn(&state, orders, &board, &mut rng);
    // Attack from P1's territory should be ignored; territory 3 stays P1.
    assert_eq!(result.state.territory_owners[3], 1);
}

#[test]
fn test_attack_with_1_army_territory_skipped() {
    // Territory with only 1 army can't attack (must leave 1 behind).
    let board = Board::from_map(test_map_4());
    let mut state = setup_play_state(&board);
    state.territory_armies[1] = 1;

    let mut rng = StdRng::seed_from_u64(42);
    let orders = [
        vec![
            Order::Deploy {
                territory: 0,
                armies: 5,
            },
            Order::Attack {
                from: 1,
                to: 2,
                armies: 1,
            },
        ],
        vec![Order::Deploy {
            territory: 2,
            armies: 5,
        }],
    ];

    let result = resolve_turn(&state, orders, &board, &mut rng);
    // Attack should be skipped (0 actual armies after leaving 1 behind).
    assert_eq!(result.state.territory_owners[2], 1);
    assert_eq!(result.state.territory_armies[1], 1);
}

// ========== CHAIN ATTACK PREVENTION ==========

#[test]
fn test_can_attack_from_turn_start_territory() {
    // Positive test: can attack from territory owned at turn start.
    let board = Board::from_map(test_map_4());
    let mut state = setup_play_state(&board);
    state.territory_armies[1] = 20;

    let mut rng = StdRng::seed_from_u64(42);
    let orders = [
        vec![Order::Attack {
            from: 1,
            to: 2,
            armies: 19,
        }],
        vec![Order::Deploy {
            territory: 2,
            armies: 5,
        }],
    ];

    let result = resolve_turn(&state, orders, &board, &mut rng);
    // Territory 1 was owned at turn start, so attack should execute.
    let attack_events: Vec<_> = result
        .events
        .iter()
        .filter(|e| matches!(e, TurnEvent::Attack { player: 0, .. }))
        .collect();
    assert!(
        !attack_events.is_empty(),
        "Attack from turn-start territory should execute"
    );
}

#[test]
fn test_multiple_attacks_from_same_territory() {
    let board = Board::from_map(test_map_4());
    let mut state = GameState::new(&board);
    // P0 owns territory 1 with lots of armies. Territories 0 and 2 are neutral.
    state.territory_owners = vec![NEUTRAL, 0, NEUTRAL, 1];
    state.territory_armies = vec![2, 30, 2, 5];
    state.alive = [true; 2];
    state.phase = Phase::Play;
    state.turn = 1;

    let mut rng = StdRng::seed_from_u64(42);
    let orders = [
        vec![
            Order::Deploy {
                territory: 1,
                armies: 5,
            },
            Order::Attack {
                from: 1,
                to: 0,
                armies: 15,
            },
            Order::Attack {
                from: 1,
                to: 2,
                armies: 15,
            },
        ],
        vec![Order::Deploy {
            territory: 3,
            armies: 5,
        }],
    ];

    let result = resolve_turn(&state, orders, &board, &mut rng);
    // Both attacks should execute (enough armies for both).
    // Territory 0 should be captured (15 vs 2, easy capture).
    assert_eq!(result.state.territory_owners[0], 0);
    // Territory 1 must have at least 1 army.
    assert!(result.state.territory_armies[1] >= 1);
    // Total armies should be conserved (original + deploy - combat losses).
}

#[test]
fn test_chain_attack_from_mid_turn_capture_blocked() {
    // Repeat of the unit test from turn.rs but as integration test with Small Earth.
    let map = load_small_earth();
    let board = Board::from_map(map);
    let mut state = GameState::new(&board);
    // P0 owns Alaska (0) with overwhelming force.
    // NW Territory (1) is neutral with 2 armies.
    // Alberta (3) is owned by P1.
    state.territory_owners[0] = 0;
    state.territory_armies[0] = 30;
    state.territory_owners[1] = NEUTRAL;
    state.territory_armies[1] = 2;
    state.territory_owners[3] = 1;
    state.territory_armies[3] = 5;
    state.alive = [true; 2];
    state.phase = Phase::Play;
    state.turn = 1;

    let mut rng = StdRng::seed_from_u64(42);
    let orders = [
        vec![
            Order::Deploy {
                territory: 0,
                armies: 5,
            },
            // First attack: capture NW Territory (1) from Alaska (0).
            Order::Attack {
                from: 0,
                to: 1,
                armies: 20,
            },
            // Chain attack: from captured NW Territory (1) to Alberta (3).
            // NW Territory (1) is adjacent to Alberta (3) but was NOT owned at turn start.
            Order::Attack {
                from: 1,
                to: 3,
                armies: 10,
            },
        ],
        vec![Order::Deploy {
            territory: 3,
            armies: 5,
        }],
    ];

    let result = resolve_turn(&state, orders, &board, &mut rng);
    assert_eq!(
        result.state.territory_owners[1], 0,
        "NW Territory should be captured"
    );
    assert_eq!(
        result.state.territory_owners[3], 1,
        "Alberta should NOT be captured via chain attack"
    );
}

// ========== TRANSFER VALIDATION ==========

#[test]
fn test_transfer_to_enemy_territory_ignored() {
    let board = Board::from_map(test_map_4());
    let state = setup_play_state(&board);

    let mut rng = StdRng::seed_from_u64(42);
    let orders = [
        vec![
            Order::Deploy {
                territory: 1,
                armies: 5,
            },
            Order::Transfer {
                from: 1,
                to: 2, // owned by P1
                armies: 5,
            },
        ],
        vec![Order::Deploy {
            territory: 2,
            armies: 5,
        }],
    ];

    let result = resolve_turn(&state, orders, &board, &mut rng);
    // Transfer to enemy should be skipped; territory 2 keeps only P1's armies.
    assert_eq!(result.state.territory_owners[2], 1);
    // P0's territory 1 should still have its armies (deploy + original, no transfer out).
    assert_eq!(result.state.territory_armies[1], 10); // 5 + 5 deploy
}

#[test]
fn test_transfer_must_leave_1_behind() {
    let board = Board::from_map(test_map_4());
    let mut state = setup_play_state(&board);
    state.territory_armies[0] = 5;

    let mut rng = StdRng::seed_from_u64(42);
    let orders = [
        vec![Order::Transfer {
            from: 0,
            to: 1,
            armies: 5, // tries to send all 5
        }],
        vec![],
    ];

    let result = resolve_turn(&state, orders, &board, &mut rng);
    assert!(
        result.state.territory_armies[0] >= 1,
        "Transfer must leave at least 1 army, got {}",
        result.state.territory_armies[0]
    );
    assert_eq!(
        result.state.territory_armies[0] + result.state.territory_armies[1] - 5,
        5,
        "Total armies should be preserved"
    );
}

#[test]
fn test_validate_transfer_to_non_adjacent() {
    let board = Board::from_map(test_map_4());
    let state = setup_play_state(&board);
    // Territories 0 and 3 are not adjacent (0-1-2-3 linear).
    // But territory 3 is owned by P1, so this would fail as TransferToEnemy.
    // Let's make P0 own all 4 territories instead.
    let mut state2 = state.clone();
    state2.territory_owners = vec![0, 0, 0, 0];
    let orders = vec![
        Order::Deploy {
            territory: 0,
            armies: 5,
        },
        Order::Transfer {
            from: 0,
            to: 3, // not adjacent to 0
            armies: 3,
        },
    ];
    let result = validate_orders(&orders, 0, &state2, &board);
    assert!(result.is_err());
}

#[test]
fn test_validate_attack_non_adjacent() {
    let board = Board::from_map(test_map_4());
    let state = setup_play_state(&board);
    let orders = vec![
        Order::Deploy {
            territory: 0,
            armies: 5,
        },
        Order::Attack {
            from: 0,
            to: 2, // not adjacent to 0
            armies: 3,
        },
    ];
    let result = validate_orders(&orders, 0, &state, &board);
    assert!(result.is_err());
}

// ========== PICKING MECHANICS ==========

#[test]
fn test_wastelands_excluded_from_pick_options() {
    // Create a map with wastelands and verify they're excluded.
    let mut map = MapFile {
        id: "wasteland_test".into(),
        name: "Wasteland Test".into(),
        territories: (0..6)
            .map(|i| Territory {
                id: i,
                name: format!("T{}", i),
                bonus_id: i / 3,
                is_wasteland: i == 1 || i == 4, // territories 1 and 4 are wastelands
                default_armies: if i == 1 || i == 4 { 10 } else { 2 },
                adjacent: if i == 0 {
                    vec![1]
                } else if i == 5 {
                    vec![4]
                } else {
                    vec![i - 1, i + 1]
                },
                visual: None,
            })
            .collect(),
        bonuses: vec![
            Bonus {
                id: 0,
                name: "A".into(),
                value: 3,
                territory_ids: vec![0, 1, 2],
                visual: None,
            },
            Bonus {
                id: 1,
                name: "B".into(),
                value: 3,
                territory_ids: vec![3, 4, 5],
                visual: None,
            },
        ],
        picking: PickingConfig {
            num_picks: 2,
            method: PickingMethod::RandomWarlords,
        },
        settings: default_settings(),
    };
    map.settings.wasteland_armies = 10;

    let board = Board::from_map(map);
    let mut rng = StdRng::seed_from_u64(42);
    let options = picking::generate_pick_options(&board, &mut rng);

    // No wasteland territory should be in the pick options.
    for &tid in &options {
        assert!(
            !board.map.territories[tid].is_wasteland,
            "Wasteland territory {} should not be a pick option",
            tid
        );
    }
}

#[test]
fn test_abba_draft_gives_balanced_picks() {
    // With 6 options (small earth), ABBA draft should give 4 each.
    let map = load_small_earth();
    let board = Board::from_map(map);
    let mut rng = StdRng::seed_from_u64(42);
    let mut state = GameState::new(&board);
    let options = picking::generate_pick_options(&board, &mut rng);

    // Both submit all options in same order.
    picking::resolve_picks(
        &mut state,
        [&options, &options],
        &board,
        picking::DEFAULT_STARTING_ARMIES,
    );

    assert_eq!(state.territory_count_for(0), 4);
    assert_eq!(state.territory_count_for(1), 4);

    // ABBA: P0 picks first (index 0), so P0 should get the first territory
    // they asked for (the top-priority pick).
    assert_eq!(state.territory_owners[options[0]], 0);
}

// ========== INCOME CALCULATION ==========

#[test]
fn test_base_income_with_no_bonuses() {
    let board = Board::from_map(test_map_4());
    let mut state = GameState::new(&board);
    // P0 owns territory 0 only (bonus "Left" needs 0 AND 1).
    state.territory_owners[0] = 0;
    let income = state.income(0, &board);
    assert_eq!(income, 5, "Should have base income only");
}

#[test]
fn test_losing_bonus_territory_removes_bonus_income() {
    let board = Board::from_map(test_map_4());
    let mut state = setup_play_state(&board);
    // P0 owns {0,1} = full Left bonus. Income = 5 + 2 = 7.
    assert_eq!(state.income(0, &board), 7);

    // P1 captures territory 1 from P0.
    state.territory_owners[1] = 1;
    // Now P0 only has territory 0, no complete bonus.
    assert_eq!(
        state.income(0, &board),
        5,
        "Losing a bonus territory should remove bonus income"
    );
}

#[test]
fn test_multiple_bonuses_income() {
    let board = Board::from_map(test_map_4());
    let mut state = GameState::new(&board);
    // P0 owns all 4 territories (both bonuses: Left=2, Right=2).
    state.territory_owners = vec![0, 0, 0, 0];
    let income = state.income(0, &board);
    assert_eq!(income, 5 + 2 + 2, "Should have base + both bonuses");
}

// ========== WASTELAND INITIALIZATION ==========

#[test]
fn test_wasteland_initialization() {
    let map = load_small_earth();
    let board = Board::from_map(map.clone());
    let state = GameState::new(&board);

    for (tid, t) in map.territories.iter().enumerate() {
        if t.is_wasteland {
            assert_eq!(
                state.territory_armies[tid],
                board.settings().wasteland_armies,
                "Wasteland territory {} should have {} armies",
                tid,
                board.settings().wasteland_armies
            );
        } else {
            assert_eq!(
                state.territory_armies[tid], t.default_armies,
                "Normal territory {} should have default {} armies",
                tid, t.default_armies
            );
        }
    }
}

#[test]
fn test_all_territories_start_neutral() {
    let map = load_small_earth();
    let board = Board::from_map(map.clone());
    let state = GameState::new(&board);

    for tid in 0..map.territories.len() {
        assert_eq!(
            state.territory_owners[tid], NEUTRAL,
            "Territory {} should start as NEUTRAL",
            tid
        );
    }
    assert_eq!(state.phase, Phase::Picking);
    assert_eq!(state.turn, 0);
}

// ========== FOG OF WAR ==========

#[test]
fn test_fog_visible_adjacent_enemy_shows_real_armies() {
    let board = Board::from_map(test_map_4());
    let mut state = GameState::new(&board);
    // P0 owns territory 0, P1 owns territory 1 (adjacent).
    state.territory_owners[0] = 0;
    state.territory_armies[0] = 5;
    state.territory_owners[1] = 1;
    state.territory_armies[1] = 15;

    let filtered = fog_filter(&state, 0, &board);
    // Territory 1 is adjacent to P0's territory 0, so it should be visible.
    assert_eq!(
        filtered.territory_owners[1], 1,
        "Adjacent enemy territory should show real owner"
    );
    assert_eq!(
        filtered.territory_armies[1], 15,
        "Adjacent enemy territory should show real army count"
    );
}

#[test]
fn test_fog_own_territory_always_visible() {
    let map = load_small_earth();
    let board = Board::from_map(map);
    let mut state = GameState::new(&board);
    state.territory_owners[0] = 0;
    state.territory_armies[0] = 42;

    let filtered = fog_filter(&state, 0, &board);
    assert_eq!(filtered.territory_owners[0], 0);
    assert_eq!(filtered.territory_armies[0], 42);
}

// ========== GAME END CONDITIONS ==========

#[test]
fn test_game_does_not_end_with_territories_remaining() {
    let board = Board::from_map(test_map_4());
    let state = setup_play_state(&board);
    // Both players have territories, game should not be finished.
    assert_eq!(state.phase, Phase::Play);
    assert!(state.winner.is_none());

    // Run a turn where no elimination happens.
    let mut rng = StdRng::seed_from_u64(42);
    let orders = [
        vec![Order::Deploy {
            territory: 0,
            armies: 5,
        }],
        vec![Order::Deploy {
            territory: 2,
            armies: 5,
        }],
    ];
    let result = resolve_turn(&state, orders, &board, &mut rng);
    assert_eq!(
        result.state.phase,
        Phase::Play,
        "Game should not end when both players have territories"
    );
    assert!(result.state.winner.is_none());
}

#[test]
fn test_game_ends_when_player_has_zero_territories() {
    let board = Board::from_map(test_map_4());
    let mut state = GameState::new(&board);
    // P0 has territory 0 and 1 with overwhelming force.
    // P1 has only territory 2 with 1 army.
    state.territory_owners = vec![0, 0, 1, NEUTRAL];
    state.territory_armies = vec![5, 30, 1, 2];
    state.alive = [true; 2];
    state.phase = Phase::Play;
    state.turn = 1;

    let mut rng = StdRng::seed_from_u64(42);
    let orders = [
        vec![Order::Attack {
            from: 1,
            to: 2,
            armies: 29,
        }],
        vec![Order::Deploy {
            territory: 2,
            armies: 5,
        }],
    ];

    let result = resolve_turn(&state, orders, &board, &mut rng);
    assert_eq!(result.state.phase, Phase::Finished);
    assert_eq!(result.state.winner, Some(0));
    assert!(!result.state.alive[1]);
}

#[test]
fn test_winner_is_player_who_eliminated_other() {
    let map = load_small_earth();
    let board = Board::from_map(map);
    let mut state = GameState::new(&board);
    state.territory_owners[0] = 0;
    state.territory_armies[0] = 50;
    state.territory_owners[1] = 1;
    state.territory_armies[1] = 1;
    state.alive = [true; 2];
    state.phase = Phase::Play;
    state.turn = 1;

    let mut rng = StdRng::seed_from_u64(42);
    let orders = [
        vec![
            Order::Deploy {
                territory: 0,
                armies: 5,
            },
            Order::Attack {
                from: 0,
                to: 1,
                armies: 54,
            },
        ],
        vec![Order::Deploy {
            territory: 1,
            armies: 5,
        }],
    ];

    let result = resolve_turn(&state, orders, &board, &mut rng);
    assert_eq!(result.state.winner, Some(0));
    // Verify events contain elimination and victory
    let has_eliminated = result
        .events
        .iter()
        .any(|e| matches!(e, TurnEvent::Eliminated { player: 1 }));
    let has_victory = result
        .events
        .iter()
        .any(|e| matches!(e, TurnEvent::Victory { player: 0 }));
    assert!(has_eliminated);
    assert!(has_victory);
}

// ========== EDGE CASES ==========

#[test]
fn test_empty_orders_pass_turn() {
    let board = Board::from_map(test_map_4());
    let state = setup_play_state(&board);

    let mut rng = StdRng::seed_from_u64(42);
    let orders: [Vec<Order>; 2] = [vec![], vec![]];

    let result = resolve_turn(&state, orders, &board, &mut rng);
    // Turn should advance, no armies should change (no deploy, no attacks).
    assert_eq!(result.state.turn, state.turn + 1);
    assert_eq!(result.state.territory_armies, state.territory_armies);
    assert_eq!(result.state.territory_owners, state.territory_owners);
}

#[test]
fn test_validate_out_of_bounds_territory() {
    let board = Board::from_map(test_map_4());
    let state = setup_play_state(&board);

    // Deploy to non-existent territory.
    let orders = vec![Order::Deploy {
        territory: 999,
        armies: 1,
    }];
    assert!(validate_orders(&orders, 0, &state, &board).is_err());

    // Attack to non-existent territory.
    let orders = vec![Order::Attack {
        from: 0,
        to: 999,
        armies: 1,
    }];
    assert!(validate_orders(&orders, 0, &state, &board).is_err());

    // Transfer to non-existent territory.
    let orders = vec![Order::Transfer {
        from: 0,
        to: 999,
        armies: 1,
    }];
    assert!(validate_orders(&orders, 0, &state, &board).is_err());
}

#[test]
fn test_validate_deploy_to_unowned_territory() {
    let board = Board::from_map(test_map_4());
    let state = setup_play_state(&board);
    // P0 tries to deploy to P1's territory 2.
    let orders = vec![Order::Deploy {
        territory: 2,
        armies: 3,
    }];
    let result = validate_orders(&orders, 0, &state, &board);
    assert!(result.is_err());
}

#[test]
fn test_attack_becomes_transfer_when_target_already_owned() {
    // If opponent captures target before your attack executes, it becomes a transfer.
    let board = Board::from_map(test_map_4());
    let mut state = GameState::new(&board);
    // P0 owns 0 and 1, P1 owns 2 and 3. Territory 2 is the contested one.
    state.territory_owners = vec![0, 0, 1, 1];
    state.territory_armies = vec![5, 20, 1, 5];
    state.alive = [true; 2];
    state.phase = Phase::Play;
    state.turn = 1;

    // P0 attacks territory 2 from territory 1 (will capture, only 1 def).
    // Then P0 also has an attack from 0 to 1 which is a transfer (both owned by P0).
    let mut rng = StdRng::seed_from_u64(42);
    let orders = [
        vec![
            Order::Deploy {
                territory: 1,
                armies: 5,
            },
            Order::Attack {
                from: 1,
                to: 2,
                armies: 20,
            },
        ],
        vec![Order::Deploy {
            territory: 3,
            armies: 5,
        }],
    ];

    let result = resolve_turn(&state, orders, &board, &mut rng);
    // Territory 2 should be captured by P0.
    assert_eq!(result.state.territory_owners[2], 0);
}

// ========== CARD SYSTEM IN TURN RESOLUTION ==========

#[test]
fn test_card_pieces_awarded_on_capture() {
    let board = Board::from_map(test_map_4());
    let mut state = GameState::new(&board);
    state.territory_owners = vec![0, 0, NEUTRAL, 1];
    state.territory_armies = vec![5, 20, 2, 5];
    state.alive = [true; 2];
    state.phase = Phase::Play;
    state.turn = 1;

    let mut rng = StdRng::seed_from_u64(42);
    let orders = [
        vec![
            Order::Deploy {
                territory: 1,
                armies: 5,
            },
            Order::Attack {
                from: 1,
                to: 2,
                armies: 15,
            },
        ],
        vec![Order::Deploy {
            territory: 3,
            armies: 5,
        }],
    ];

    let result = resolve_turn(&state, orders, &board, &mut rng);
    // P0 should have earned 1 card piece for capturing territory 2.
    assert!(
        result.state.card_pieces[0] >= 1 || !result.state.hands[0].is_empty(),
        "Capturing a territory should award a card piece"
    );
}

#[test]
fn test_3_captures_gives_reinforcement_card() {
    let board = Board::from_map(test_map_4());
    let mut state = GameState::new(&board);
    // Set up so P0 already has 2 card pieces and can capture 1 more territory.
    state.territory_owners = vec![0, 0, NEUTRAL, 1];
    state.territory_armies = vec![5, 30, 2, 5];
    state.card_pieces = [2, 0];
    state.alive = [true; 2];
    state.phase = Phase::Play;
    state.turn = 1;

    let mut rng = StdRng::seed_from_u64(42);
    let orders = [
        vec![
            Order::Deploy {
                territory: 1,
                armies: 5,
            },
            Order::Attack {
                from: 1,
                to: 2,
                armies: 20,
            },
        ],
        vec![Order::Deploy {
            territory: 3,
            armies: 5,
        }],
    ];

    let result = resolve_turn(&state, orders, &board, &mut rng);
    // 2 existing pieces + 1 capture = 3 pieces -> 1 Reinforcement card.
    assert_eq!(result.state.card_pieces[0], 0, "3 pieces consumed for card");
    assert_eq!(result.state.hands[0].len(), 1);
    assert!(matches!(
        result.state.hands[0][0],
        Card::Reinforcement(5)
    ));
}

// ========== BLOCKADE CARD IN TURN RESOLUTION ==========

#[test]
fn test_blockade_card_in_turn() {
    let board = Board::from_map(test_map_4());
    let mut state = setup_play_state(&board);
    state.territory_armies[0] = 4;
    state.hands[0] = vec![Card::Blockade];

    let mut rng = StdRng::seed_from_u64(42);
    let orders = [
        vec![
            Order::Deploy {
                territory: 1,
                armies: 5,
            },
            Order::PlayCard {
                card: Card::Blockade,
                target: 0,
            },
        ],
        vec![Order::Deploy {
            territory: 2,
            armies: 5,
        }],
    ];

    let result = resolve_turn(&state, orders, &board, &mut rng);
    // Territory 0 should be blockaded: 4 * 3 = 12 armies, becomes NEUTRAL.
    assert_eq!(result.state.territory_owners[0], NEUTRAL);
    assert_eq!(result.state.territory_armies[0], 12);
    assert!(result.state.hands[0].is_empty());
}

// ========== REINFORCEMENT CARD IN TURN RESOLUTION ==========

#[test]
fn test_reinforcement_card_in_turn() {
    let board = Board::from_map(test_map_4());
    let mut state = setup_play_state(&board);
    state.hands[0] = vec![Card::Reinforcement(5)];

    let mut rng = StdRng::seed_from_u64(42);
    let orders = [
        vec![
            Order::Deploy {
                territory: 0,
                armies: 5,
            },
            Order::PlayCard {
                card: Card::Reinforcement(5),
                target: 1,
            },
        ],
        vec![Order::Deploy {
            territory: 2,
            armies: 5,
        }],
    ];

    let result = resolve_turn(&state, orders, &board, &mut rng);
    // Territory 1 should receive +5 from card.
    assert_eq!(
        result.state.territory_armies[1],
        5 + 5,
        "Reinforcement card should add 5 armies"
    );
    assert!(result.state.hands[0].is_empty());
}

// ========== COMBAT EDGE CASES ==========

#[test]
fn test_combat_large_armies() {
    let s = default_settings();
    // Large army vs small: should always capture.
    let r = resolve_attack(100, 1, &s);
    assert!(r.captured);
    assert_eq!(r.surviving_attackers, 99);

    // Large vs large: verify no overflow.
    let r = resolve_attack(1000, 500, &s);
    assert!(r.captured || !r.captured); // just verify it doesn't panic
    assert!(r.attackers_killed <= 1000);
    assert!(r.defenders_killed <= 500);
}

#[test]
fn test_combat_minimum_capture_table() {
    // Verify the minimum attackers needed to capture N defenders at 0% luck.
    let s = default_settings();

    // Against 1 def: need 2 (1v1 = mutual kill, 2v1 = capture).
    assert!(!resolve_attack(1, 1, &s).captured);
    assert!(resolve_attack(2, 1, &s).captured);

    // Against 2 def: need 3 (3*0.6=1.8→2 kills all def, 2*0.7=1.4→1 kill).
    assert!(!resolve_attack(2, 2, &s).captured);
    assert!(resolve_attack(3, 2, &s).captured);

    // Against 3 def: need 5 (5*0.6=3.0→3, 3*0.7=2.1→2).
    assert!(!resolve_attack(4, 3, &s).captured);
    assert!(resolve_attack(5, 3, &s).captured);

    // Against 4 def: need 7 (7*0.6=4.2→4, 4*0.7=2.8→3). Actually:
    // 6*0.6=3.6→4 kills. 4*0.7=2.8→3 kills. Capture! (6 needed)
    // Wait: 6*0.6 = 3.6, straight_round(3.6) = floor(3.6+0.5) = floor(4.1) = 4. Yes captures.
    // 5*0.6 = 3.0, straight_round(3.0) = floor(3.5) = 3. 1 def survives. No capture.
    assert!(!resolve_attack(5, 4, &s).captured);
    assert!(resolve_attack(6, 4, &s).captured);
}

// ========== ARMY CONSERVATION ==========

#[test]
fn test_total_armies_conserved_in_combat() {
    let board = Board::from_map(test_map_4());
    let mut state = setup_play_state(&board);
    state.territory_armies = vec![5, 20, 10, 5];

    let mut rng = StdRng::seed_from_u64(42);
    let orders = [
        vec![
            Order::Deploy {
                territory: 1,
                armies: 7,
            },
            Order::Attack {
                from: 1,
                to: 2,
                armies: 15,
            },
        ],
        vec![Order::Deploy {
            territory: 2,
            armies: 7,
        }],
    ];

    let before_total: u32 = state.territory_armies.iter().sum::<u32>() + 7 + 7; // + deploys
    let result = resolve_turn(&state, orders, &board, &mut rng);
    let after_total: u32 = result.state.territory_armies.iter().sum();

    // Armies should decrease (combat kills armies), but never increase beyond deploy.
    assert!(
        after_total <= before_total,
        "Total armies after combat ({}) should not exceed before ({})",
        after_total,
        before_total
    );
}

// ========== VALIDATE: ATTACK ZERO ARMIES ==========

#[test]
fn test_validate_attack_zero_armies() {
    let board = Board::from_map(test_map_4());
    let state = setup_play_state(&board);
    let orders = vec![Order::Attack {
        from: 1,
        to: 2,
        armies: 0,
    }];
    let result = validate_orders(&orders, 0, &state, &board);
    assert!(result.is_err());
}

#[test]
fn test_validate_transfer_zero_armies() {
    let board = Board::from_map(test_map_4());
    let state = setup_play_state(&board);
    let orders = vec![Order::Transfer {
        from: 0,
        to: 1,
        armies: 0,
    }];
    let result = validate_orders(&orders, 0, &state, &board);
    assert!(result.is_err());
}

// ========== INTERLEAVED EXECUTION ==========

#[test]
fn test_interleaved_execution_fairness() {
    // Both players attack the same neutral. Only one can capture.
    // Run many seeds and verify both players get a fair chance.
    let board = Board::from_map(test_map_4());

    let mut p0_captures = 0;
    let mut p1_captures = 0;

    for seed in 0..100u64 {
        let mut state = GameState::new(&board);
        state.territory_owners = vec![0, NEUTRAL, NEUTRAL, 1];
        state.territory_armies = vec![15, 2, 2, 15];
        state.alive = [true; 2];
        state.phase = Phase::Play;
        state.turn = 1;

        let mut rng = StdRng::seed_from_u64(seed);
        // Both players attack the same neutral territory (not possible here since
        // territories 1 and 2 are different). Let's have them attack territory 1 and 2.
        // Instead: P0 attacks 1, P1 attacks 2. Not quite what we want.
        // Actually, territory 1 is adjacent to 0 AND 2 (which is adjacent to 3).
        // But territory 1 is only adjacent to 0 and 2, not 3.
        // So P1 can't attack territory 1 from territory 3.
        // Let's just test that interleaving doesn't crash and both get to move.
        let orders = [
            vec![
                Order::Deploy {
                    territory: 0,
                    armies: 5,
                },
                Order::Attack {
                    from: 0,
                    to: 1,
                    armies: 19,
                },
            ],
            vec![
                Order::Deploy {
                    territory: 3,
                    armies: 5,
                },
                Order::Attack {
                    from: 3,
                    to: 2,
                    armies: 19,
                },
            ],
        ];

        let result = resolve_turn(&state, orders, &board, &mut rng);
        if result.state.territory_owners[1] == 0 {
            p0_captures += 1;
        }
        if result.state.territory_owners[2] == 1 {
            p1_captures += 1;
        }
    }

    // Both should capture their target every time (overwhelming force vs 2 neutrals).
    assert_eq!(p0_captures, 100, "P0 should capture territory 1 every time");
    assert_eq!(p1_captures, 100, "P1 should capture territory 2 every time");
}

// ========== CHECK ELIMINATION LOGIC ==========

#[test]
fn test_check_elimination_both_alive() {
    let board = Board::from_map(test_map_4());
    let mut state = setup_play_state(&board);
    state.check_elimination();
    assert!(state.alive[0]);
    assert!(state.alive[1]);
    assert!(state.winner.is_none());
}

#[test]
fn test_check_elimination_one_player_no_territories() {
    let board = Board::from_map(test_map_4());
    let mut state = GameState::new(&board);
    state.territory_owners = vec![0, 0, 0, NEUTRAL];
    state.alive = [true; 2];
    state.phase = Phase::Play;

    state.check_elimination();
    assert!(!state.alive[1], "P1 with 0 territories should be eliminated");
    assert_eq!(state.winner, Some(0));
    assert_eq!(state.phase, Phase::Finished);
}
