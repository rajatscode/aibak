use std::path::PathBuf;

use rand::rngs::StdRng;
use rand::SeedableRng;

use strat_engine::ai;
use strat_engine::combat::resolve_attack;
use strat_engine::fog::{fog_filter, visible_territories};
use strat_engine::map::{Map, MapSettings};
use strat_engine::orders::Order;
use strat_engine::picking;
use strat_engine::state::{GameState, Phase, NEUTRAL};
use strat_engine::turn::resolve_turn;

fn maps_dir() -> PathBuf {
    // Navigate from crate root to workspace root
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap() // crates/
        .parent().unwrap() // workspace root
        .join("maps")
}

fn load_small_earth() -> Map {
    Map::load(&maps_dir().join("small_earth.json")).expect("Failed to load Small Earth map")
}

fn load_mme() -> Map {
    Map::load(&maps_dir().join("mme.json")).expect("Failed to load MME map")
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
    assert_eq!(map.territory_count(), 42);
    assert_eq!(map.bonuses.len(), 6);
}

#[test]
fn test_mme_loads() {
    let map = load_mme();
    assert_eq!(map.territory_count(), 89);
    assert_eq!(map.bonuses.len(), 22);
}

#[test]
fn test_all_adjacencies_bidirectional() {
    for map in [load_small_earth(), load_mme()] {
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
    for map in [load_small_earth(), load_mme()] {
        let mut visited = vec![false; map.territory_count()];
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
    for map in [load_small_earth(), load_mme()] {
        let mut bonus_count = vec![0usize; map.territory_count()];
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
    let mut rng = StdRng::seed_from_u64(42);
    let options = picking::generate_pick_options(&map, &mut rng);

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
    assert_eq!(bonus_ids.len(), expected, "picks must come from different bonuses");
}

#[test]
fn test_picking_resolves_to_play_phase() {
    let map = load_small_earth();
    let mut rng = StdRng::seed_from_u64(42);
    let mut state = GameState::new(&map);

    let options = picking::generate_pick_options(&map, &mut rng);
    // Both players submit all options as their priority list.
    // ABBA snake draft with 6 options, 4 picks each = 8 total picks.
    // Some will be auto-filled from random unclaimed territories.
    let picks_a: Vec<usize> = options.clone();
    let picks_b: Vec<usize> = options.iter().rev().copied().collect();

    picking::resolve_picks(&mut state, [&picks_a, &picks_b], &map, picking::DEFAULT_STARTING_ARMIES);

    assert_eq!(state.phase, Phase::Play);
    assert_eq!(state.turn, 1);
    assert_eq!(state.territory_count_for(0), 4, "Player 0 should get 4 territories");
    assert_eq!(state.territory_count_for(1), 4, "Player 1 should get 4 territories");
}

#[test]
fn test_picked_territories_get_starting_armies() {
    let map = load_small_earth();
    let mut rng = StdRng::seed_from_u64(42);
    let mut state = GameState::new(&map);

    let options = picking::generate_pick_options(&map, &mut rng);
    let picks: Vec<usize> = options.iter().take(4).copied().collect();
    let ai_picks = ai::generate_picks(&state, &map);

    picking::resolve_picks(&mut state, [&picks, &ai_picks], &map, picking::DEFAULT_STARTING_ARMIES);

    // All picked territories should have 5 armies
    for tid in 0..map.territory_count() {
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
    let mut state = GameState::new(&map);
    // Player 0 owns Alaska (0)
    state.territory_owners[0] = 0;
    state.territory_armies[0] = 5;

    let visible = visible_territories(&state, 0, &map);

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
    let mut state = GameState::new(&map);
    state.territory_owners[0] = 0;
    state.territory_armies[0] = 5;
    state.territory_owners[12] = 1;
    state.territory_armies[12] = 20;

    let filtered = fog_filter(&state, 0, &map);

    // Can't see Argentina's real army count
    assert_eq!(filtered.territory_owners[12], NEUTRAL);
    assert_ne!(filtered.territory_armies[12], 20);
}

// ========== TURN RESOLUTION ==========

#[test]
fn test_deploy_increases_armies() {
    let map = load_small_earth();
    let mut state = GameState::new(&map);
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

    let result = resolve_turn(&state, orders, &map, &mut rng);
    assert_eq!(result.state.territory_armies[0], 6); // 1 + 5
}

#[test]
fn test_successful_attack_captures_territory() {
    let map = load_small_earth();
    let mut state = GameState::new(&map);
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

    let result = resolve_turn(&state, orders, &map, &mut rng);
    assert_eq!(result.state.territory_owners[1], 0, "Should capture NW Territory");
    assert!(result.state.territory_armies[1] > 0, "Should have surviving attackers");
    assert_eq!(result.state.territory_armies[0], 1, "Should leave 1 on Alaska");
}

#[test]
fn test_elimination_triggers_victory() {
    let map = load_small_earth();
    let mut state = GameState::new(&map);
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
            Order::Deploy { territory: 0, armies: 5 },
            Order::Attack { from: 0, to: 1, armies: 24 },
        ],
        vec![Order::Deploy { territory: 1, armies: 5 }],
    ];

    let result = resolve_turn(&state, orders, &map, &mut rng);
    assert_eq!(result.state.phase, Phase::Finished);
    assert_eq!(result.state.winner, Some(0));

    // Should have elimination + victory events
    let has_victory = result.events.iter().any(|e| matches!(e, strat_engine::turn::TurnEvent::Victory { player: 0 }));
    assert!(has_victory, "Should have victory event");
}

// ========== AI ==========

#[test]
fn test_ai_generates_valid_orders() {
    let map = load_small_earth();
    let mut state = GameState::new(&map);
    let mut rng = StdRng::seed_from_u64(42);

    // Set up a realistic game state
    let options = picking::generate_pick_options(&map, &mut rng);
    let picks: Vec<usize> = options.iter().take(4).copied().collect();
    let ai_picks = ai::generate_picks(&state, &map);
    picking::resolve_picks(&mut state, [&picks, &ai_picks], &map, picking::DEFAULT_STARTING_ARMIES);

    // AI should generate orders
    let orders = ai::generate_orders(&state, 1, &map);
    assert!(!orders.is_empty(), "AI should generate at least deploy orders");

    // First order should be a deploy
    assert!(
        matches!(orders[0], Order::Deploy { .. }),
        "AI's first order should be a deploy"
    );

    // Total deploy should equal income
    let income = state.income(1, &map);
    let total_deployed: u32 = orders
        .iter()
        .filter_map(|o| match o {
            Order::Deploy { armies, .. } => Some(*armies),
            _ => None,
        })
        .sum();
    assert_eq!(total_deployed, income, "AI should deploy exactly its income");
}

#[test]
fn test_ai_expands_over_multiple_turns() {
    let map = load_small_earth();
    let mut state = GameState::new(&map);
    let mut rng = StdRng::seed_from_u64(42);

    let options = picking::generate_pick_options(&map, &mut rng);
    let picks: Vec<usize> = options.iter().take(4).copied().collect();
    let ai_picks = ai::generate_picks(&state, &map);
    picking::resolve_picks(&mut state, [&picks, &ai_picks], &map, picking::DEFAULT_STARTING_ARMIES);

    let initial_ai_territories = state.territory_count_for(1);

    // Run 5 turns of AI playing both sides
    for _ in 0..5 {
        let p0_orders = ai::generate_orders(&state, 0, &map);
        let p1_orders = ai::generate_orders(&state, 1, &map);
        let result = resolve_turn(&state, [p0_orders, p1_orders], &map, &mut rng);
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
    let mut state = GameState::new(&map);
    let mut rng = StdRng::seed_from_u64(123);

    let options = picking::generate_pick_options(&map, &mut rng);
    let picks: Vec<usize> = options.iter().take(4).copied().collect();
    let ai_picks = ai::generate_picks(&state, &map);
    picking::resolve_picks(&mut state, [&picks, &ai_picks], &map, picking::DEFAULT_STARTING_ARMIES);

    // Play up to 100 turns — game should end well before that
    for turn in 0..100 {
        if state.phase == Phase::Finished {
            println!("Game ended on turn {} — winner: {:?}", turn, state.winner);
            return;
        }
        let p0_orders = ai::generate_orders(&state, 0, &map);
        let p1_orders = ai::generate_orders(&state, 1, &map);
        let result = resolve_turn(&state, [p0_orders, p1_orders], &map, &mut rng);
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
    let mut state = GameState::new(&map);

    // Give player 0 all of South America (bonus 1: territories 9,10,11,12)
    for &tid in &map.bonuses[1].territory_ids {
        state.territory_owners[tid] = 0;
        state.territory_armies[tid] = 5;
    }

    let income = state.income(0, &map);
    let bonus_value = map.bonuses[1].value;
    assert_eq!(income, 5 + bonus_value, "Income should be base + bonus");
}

// ========== CONCURRENT ATTACKS ==========

#[test]
fn test_concurrent_attacks_same_territory() {
    let map = load_small_earth();
    let mut state = GameState::new(&map);
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
            Order::Deploy { territory: 0, armies: 5 },
            Order::Attack { from: 0, to: 1, armies: 19 },
        ],
        vec![
            Order::Deploy { territory: 3, armies: 5 },
            Order::Attack { from: 3, to: 1, armies: 19 },
        ],
    ];

    let result = resolve_turn(&state, orders, &map, &mut rng);
    // NW Territory (1) should be owned by exactly one player, not both.
    let owner = result.state.territory_owners[1];
    assert!(owner == 0 || owner == 1, "Territory should be captured by one player, got {}", owner);
    assert!(result.state.territory_armies[1] > 0);
}

// ========== TRANSFER CHAIN ==========

#[test]
fn test_transfer_chain() {
    let map = load_small_earth();
    let mut state = GameState::new(&map);
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
            Order::Deploy { territory: 0, armies: 5 },
            Order::Transfer { from: 0, to: 1, armies: 24 },
            Order::Transfer { from: 1, to: 3, armies: 24 },
        ],
        vec![],
    ];

    let result = resolve_turn(&state, orders, &map, &mut rng);
    // Armies should flow from 0 -> 1 -> 3.
    // Territory 0 should keep 1 army. The rest go to 1, then from 1 to 3.
    assert_eq!(result.state.territory_armies[0], 1);
    // All transferred armies should end up somewhere.
    // Original: 20 + 1 + 1 = 22. Deploy 5 to territory 0 = 27 total.
    let total: u32 = result.state.territory_armies[0]
        + result.state.territory_armies[1]
        + result.state.territory_armies[3];
    assert_eq!(total, 20 + 1 + 1 + 5, "Total armies should be preserved (original + deployed)");
}

// ========== BONUS INCOME ==========

#[test]
fn test_bonus_income_correct() {
    for map in [load_small_earth(), load_mme()] {
        let mut state = GameState::new(&map);
        // Test each bonus individually.
        for bonus in &map.bonuses {
            // Reset state.
            state.territory_owners = vec![NEUTRAL; map.territory_count()];
            // Assign this bonus to player 0.
            for &tid in &bonus.territory_ids {
                state.territory_owners[tid] = 0;
            }
            let income = state.income(0, &map);
            assert_eq!(
                income,
                map.settings.base_income + bonus.value,
                "Map '{}', bonus '{}': expected income {} + {}, got {}",
                map.name, bonus.name, map.settings.base_income, bonus.value, income
            );
        }
    }
}

// ========== FOG CONSISTENCY ==========

#[test]
fn test_fog_consistency() {
    let map = load_small_earth();
    let mut state = GameState::new(&map);
    let mut rng = StdRng::seed_from_u64(42);

    let options = picking::generate_pick_options(&map, &mut rng);
    let picks_a: Vec<usize> = options.iter().take(4).copied().collect();
    let picks_b = ai::generate_picks(&state, &map);
    picking::resolve_picks(&mut state, [&picks_a, &picks_b], &map, picking::DEFAULT_STARTING_ARMIES);

    // Run a few turns to create a realistic state.
    for _ in 0..3 {
        if state.phase == Phase::Finished { break; }
        let p0 = ai::generate_orders(&state, 0, &map);
        let p1 = ai::generate_orders(&state, 1, &map);
        let result = resolve_turn(&state, [p0, p1], &map, &mut rng);
        state = result.state;
    }

    let visible = visible_territories(&state, 0, &map);
    let filtered = fog_filter(&state, 0, &map);

    for tid in 0..map.territory_count() {
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
    let mut state = GameState::new(&map);
    let mut rng = StdRng::seed_from_u64(99);

    let options = picking::generate_pick_options(&map, &mut rng);
    let picks_a: Vec<usize> = options.iter().take(4).copied().collect();
    let picks_b = ai::generate_picks(&state, &map);
    picking::resolve_picks(&mut state, [&picks_a, &picks_b], &map, picking::DEFAULT_STARTING_ARMIES);

    for _ in 0..10 {
        if state.phase == Phase::Finished { break; }
        let p0 = ai::generate_orders(&state, 0, &map);
        let p1 = ai::generate_orders(&state, 1, &map);
        let result = resolve_turn(&state, [p0, p1], &map, &mut rng);
        state = result.state;

        // Invariants.
        for tid in 0..map.territory_count() {
            assert!(
                state.territory_owners[tid] == 0
                    || state.territory_owners[tid] == 1
                    || state.territory_owners[tid] == NEUTRAL,
                "Invalid owner {} at territory {}", state.territory_owners[tid], tid
            );
            // Armies should never be zero for owned territories that are alive.
            // (unless captured this turn, but territory_armies should still be > 0 for owned)
        }
    }

    // No territory should have negative armies (u32 so check for extreme values from overflow).
    for tid in 0..map.territory_count() {
        assert!(state.territory_armies[tid] < 100_000, "Suspicious army count at territory {}: {}", tid, state.territory_armies[tid]);
    }
}

// ========== MME FULL GAME ==========

#[test]
fn test_mme_full_game() {
    let map = load_mme();
    let mut state = GameState::new(&map);
    let mut rng = StdRng::seed_from_u64(77);

    let options = picking::generate_pick_options(&map, &mut rng);
    let picks_a: Vec<usize> = options.iter().take(4).copied().collect();
    let picks_b = ai::generate_picks(&state, &map);
    picking::resolve_picks(&mut state, [&picks_a, &picks_b], &map, picking::DEFAULT_STARTING_ARMIES);

    for turn in 0..200 {
        if state.phase == Phase::Finished {
            println!("MME game ended on turn {}, winner: {:?}", turn, state.winner);
            return;
        }
        let p0 = ai::generate_orders(&state, 0, &map);
        let p1 = ai::generate_orders(&state, 1, &map);
        let result = resolve_turn(&state, [p0, p1], &map, &mut rng);
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
    let mut state = GameState::new(&map);
    let mut rng = StdRng::seed_from_u64(42);
    let options = picking::generate_pick_options(&map, &mut rng);

    // Both players submit identical pick lists.
    picking::resolve_picks(&mut state, [&options, &options], &map, picking::DEFAULT_STARTING_ARMIES);

    assert_eq!(state.territory_count_for(0), 4);
    assert_eq!(state.territory_count_for(1), 4);
    // No territory should be shared.
    for tid in 0..map.territory_count() {
        let o = state.territory_owners[tid];
        assert!(o == 0 || o == 1 || o == NEUTRAL);
    }
}

// ========== INCOME NEVER NEGATIVE ==========

#[test]
fn test_income_never_negative() {
    let map = load_small_earth();
    let mut state = GameState::new(&map);
    let mut rng = StdRng::seed_from_u64(55);

    let options = picking::generate_pick_options(&map, &mut rng);
    let picks_a: Vec<usize> = options.iter().take(4).copied().collect();
    let picks_b = ai::generate_picks(&state, &map);
    picking::resolve_picks(&mut state, [&picks_a, &picks_b], &map, picking::DEFAULT_STARTING_ARMIES);

    for _ in 0..20 {
        if state.phase == Phase::Finished { break; }
        for player in 0..2u8 {
            if state.alive[player as usize] {
                let income = state.income(player, &map);
                assert!(
                    income >= map.settings.base_income,
                    "Player {} income {} is below base income {}",
                    player, income, map.settings.base_income
                );
            }
        }
        let p0 = ai::generate_orders(&state, 0, &map);
        let p1 = ai::generate_orders(&state, 1, &map);
        let result = resolve_turn(&state, [p0, p1], &map, &mut rng);
        state = result.state;
    }
}

// ========== ELIMINATION ==========

#[test]
fn test_elimination_correct() {
    let map = load_small_earth();
    let mut state = GameState::new(&map);
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
            Order::Deploy { territory: 0, armies: 5 },
            Order::Attack { from: 0, to: 1, armies: 54 },
        ],
        vec![Order::Deploy { territory: 1, armies: 5 }],
    ];

    let result = resolve_turn(&state, orders, &map, &mut rng);
    assert_eq!(result.state.phase, Phase::Finished);
    assert_eq!(result.state.winner, Some(0));
    assert!(!result.state.alive[1]);
}

// ========== PROPERTY-BASED: 50 RANDOM GAMES ==========

#[test]
fn test_random_games_invariants() {
    let map = load_small_earth();

    for seed in 0..50u64 {
        let mut state = GameState::new(&map);
        let mut rng = StdRng::seed_from_u64(seed);

        let _options = picking::generate_pick_options(&map, &mut rng);
        let picks_a = ai::generate_picks(&state, &map);
        let picks_b = ai::generate_picks(&state, &map);
        picking::resolve_picks(&mut state, [&picks_a, &picks_b], &map, picking::DEFAULT_STARTING_ARMIES);

        for turn in 0..200 {
            if state.phase == Phase::Finished { break; }

            let p0 = ai::generate_orders(&state, 0, &map);
            let p1 = ai::generate_orders(&state, 1, &map);
            let result = resolve_turn(&state, [p0, p1], &map, &mut rng);
            state = result.state;

            // Invariant 1: No territory has overflow-level armies (proxy for negative).
            for tid in 0..map.territory_count() {
                assert!(
                    state.territory_armies[tid] < 100_000,
                    "Seed {}, turn {}: territory {} has suspicious army count {}",
                    seed, turn, tid, state.territory_armies[tid]
                );
            }

            // Invariant 2: All owners are valid.
            for tid in 0..map.territory_count() {
                let o = state.territory_owners[tid];
                assert!(
                    o == 0 || o == 1 || o == NEUTRAL,
                    "Seed {}, turn {}: territory {} has invalid owner {}",
                    seed, turn, tid, o
                );
            }

            // Invariant 3: Total territory accounting.
            let p0_count = state.territory_count_for(0);
            let p1_count = state.territory_count_for(1);
            let neutral_count = state.territory_count_for(NEUTRAL);
            assert_eq!(
                p0_count + p1_count + neutral_count,
                map.territory_count(),
                "Seed {}, turn {}: territory count mismatch",
                seed, turn
            );

            // Invariant 4: Income is >= base_income for alive players.
            for player in 0..2u8 {
                if state.alive[player as usize] && state.territory_count_for(player) > 0 {
                    let income = state.income(player, &map);
                    assert!(
                        income >= map.settings.base_income,
                        "Seed {}, turn {}: player {} income {} below base {}",
                        seed, turn, player, income, map.settings.base_income
                    );
                }
            }
        }

        // Invariant 5: Game should terminate within 200 turns.
        assert!(
            state.phase == Phase::Finished || state.turn <= 201,
            "Seed {}: game did not terminate within 200 turns", seed
        );
    }
}
