//! Fog of war: visibility computation and state filtering per player.

use std::collections::HashSet;

use crate::board::Board;
use crate::map::Map;
use crate::state::{GameState, NEUTRAL, PlayerId};

/// Compute the set of territory IDs visible to a player.
/// A territory is visible if:
/// - Owned by the player, OR
/// - Adjacent to any territory owned by the player.
pub fn visible_territories(state: &GameState, player: PlayerId, board: &Board) -> HashSet<usize> {
    let map = &board.map;
    let mut visible = HashSet::new();

    for (tid, &owner) in state.territory_owners.iter().enumerate() {
        if owner == player {
            visible.insert(tid);
            for &adj in &map.territories[tid].adjacent {
                visible.insert(adj);
            }
        }
    }

    visible
}

/// Compute visibility from a raw ownership array (no GameState needed).
/// Used for incremental visibility tracking during event filtering.
pub fn visible_territories_from_owners(owners: &[PlayerId], player: PlayerId, map: &Map) -> HashSet<usize> {
    let mut visible = HashSet::new();

    for (tid, &owner) in owners.iter().enumerate() {
        if owner == player {
            visible.insert(tid);
            for &adj in &map.territories[tid].adjacent {
                visible.insert(adj);
            }
        }
    }

    visible
}

/// Create a fog-filtered copy of the game state for a player.
/// Non-visible territories show as neutral with unknown army counts.
pub fn fog_filter(state: &GameState, player: PlayerId, board: &Board) -> GameState {
    if !board.settings().fog_of_war {
        return state.clone();
    }

    let map = &board.map;
    let visible = visible_territories(state, player, board);
    let mut filtered = state.clone();

    for tid in 0..board.map.territory_count() {
        if !visible.contains(&tid) {
            filtered.territory_owners[tid] = NEUTRAL;
            // Show default army count for fogged territories.
            filtered.territory_armies[tid] = map.territories[tid].default_armies;
        }
    }

    // Don't reveal opponent's hand or card pieces.
    let opp = 1 - player as usize;
    filtered.hands[opp] = Vec::new();
    filtered.card_pieces[opp] = 0;

    filtered
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::Board;
    use crate::map::{Bonus, MapFile, MapSettings, PickingConfig, PickingMethod, Territory};

    fn test_map() -> MapFile {
        // Simple 4-territory map: 0-1-2-3 in a line.
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

    #[test]
    fn test_visibility() {
        let map = test_map();
        let board = Board::from_map(map);
        let mut state = GameState::new(&board);
        state.territory_owners[0] = 0;
        state.territory_owners[3] = 1;

        let vis0 = visible_territories(&state, 0, &board);
        assert!(vis0.contains(&0)); // owns it
        assert!(vis0.contains(&1)); // adjacent
        assert!(!vis0.contains(&2)); // not adjacent
        assert!(!vis0.contains(&3)); // not adjacent

        let vis1 = visible_territories(&state, 1, &board);
        assert!(vis1.contains(&3));
        assert!(vis1.contains(&2));
        assert!(!vis1.contains(&1));
        assert!(!vis1.contains(&0));
    }

    #[test]
    fn test_fog_filter_hides_enemy() {
        let map = test_map();
        let board = Board::from_map(map);
        let mut state = GameState::new(&board);
        state.territory_owners[0] = 0;
        state.territory_armies[0] = 5;
        state.territory_owners[3] = 1;
        state.territory_armies[3] = 8;

        let filtered = fog_filter(&state, 0, &board);
        // Player 0 can't see territory 3.
        assert_eq!(filtered.territory_owners[3], NEUTRAL);
        assert_eq!(filtered.territory_armies[3], 2); // default
        // Player 0 can see their own territory.
        assert_eq!(filtered.territory_owners[0], 0);
        assert_eq!(filtered.territory_armies[0], 5);
    }

    use crate::cards::Card;
    use crate::state::GameState;

    #[test]
    fn test_visibility_union_of_owned_territories() {
        let map = test_map();
        let board = Board::from_map(map);
        let mut state = GameState::new(&board);
        // Player 0 owns territories 0 and 3 (opposite ends of the line).
        state.territory_owners[0] = 0;
        state.territory_owners[3] = 0;

        let vis = visible_territories(&state, 0, &board);
        // From 0: sees 0, 1. From 3: sees 2, 3. Union = all 4.
        assert!(vis.contains(&0));
        assert!(vis.contains(&1));
        assert!(vis.contains(&2));
        assert!(vis.contains(&3));
        assert_eq!(vis.len(), 4);
    }

    #[test]
    fn test_fog_filter_hides_opponent_cards() {
        let map = test_map();
        let board = Board::from_map(map);
        let mut state = GameState::new(&board);
        state.territory_owners[0] = 0;
        state.territory_owners[3] = 1;
        // Give opponent cards and card pieces.
        state.hands[1] = vec![Card::Reinforcement(5), Card::Blockade];
        state.card_pieces[1] = 2;

        let filtered = fog_filter(&state, 0, &board);
        // Opponent's hand should be empty in the filtered view.
        assert!(filtered.hands[1].is_empty());
        assert_eq!(filtered.card_pieces[1], 0);
        // Player's own hand should be preserved.
        assert_eq!(filtered.hands[0], state.hands[0]);
    }

    #[test]
    fn test_visibility_on_small_earth() {
        use std::path::PathBuf;
        let maps_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("maps");
        let board = Board::from_map(MapFile::load(&maps_dir.join("small_earth.json")).expect("load map"));

        let mut state = GameState::new(&board);
        // Player 0 owns Alaska (0) and NW Territory (1).
        state.territory_owners[0] = 0;
        state.territory_owners[1] = 0;

        let vis = visible_territories(&state, 0, &board);
        // Should see owned territories.
        assert!(vis.contains(&0));
        assert!(vis.contains(&1));
        // Should see all neighbors of both.
        for &owned in &[0, 1] {
            for &adj in &board.map.territories[owned].adjacent {
                assert!(
                    vis.contains(&adj),
                    "should see neighbor {} of territory {}",
                    adj,
                    owned
                );
            }
        }
        // Should NOT see distant territories like Argentina (12).
        assert!(!vis.contains(&12));
    }

    #[test]
    fn test_visible_enemy_bonus_count_excludes_fogged() {
        // Simulates the fix: bonus enemy counts must only include visible territories.
        let map = test_map();
        let board = Board::from_map(map);
        let mut state = GameState::new(&board);
        // Player 0 owns territory 0, enemy owns territories 2 and 3 (bonus "Right").
        state.territory_owners[0] = 0;
        state.territory_owners[2] = 1;
        state.territory_owners[3] = 1;

        let visible = visible_territories(&state, 0, &board);
        // Player 0 sees: 0 (own), 1 (adjacent). Does NOT see 2 or 3.
        assert!(!visible.contains(&2));
        assert!(!visible.contains(&3));

        // Bonus "Right" has territories [2, 3], both enemy-owned but invisible.
        let right_bonus = &board.map.bonuses[1];
        let visible_enemy_count = right_bonus
            .territory_ids
            .iter()
            .filter(|&&tid| {
                let owner = state.territory_owners[tid];
                owner != 0 && owner != NEUTRAL && visible.contains(&tid)
            })
            .count();
        assert_eq!(visible_enemy_count, 0, "should not see any enemies in fogged bonus");

        // Total visible enemy territories should also be 0.
        let total_visible_enemies = visible
            .iter()
            .filter(|&&tid| state.territory_owners[tid] == 1)
            .count();
        assert_eq!(total_visible_enemies, 0);
    }

    #[test]
    fn test_visible_enemy_count_includes_seen_enemies() {
        // When enemies ARE visible, they should be counted.
        let map = test_map();
        let board = Board::from_map(map);
        let mut state = GameState::new(&board);
        // Player 0 owns territory 0, enemy owns territory 1 (adjacent, so visible).
        state.territory_owners[0] = 0;
        state.territory_owners[1] = 1;
        state.territory_owners[3] = 1;

        let visible = visible_territories(&state, 0, &board);
        assert!(visible.contains(&1), "adjacent enemy should be visible");
        assert!(!visible.contains(&3), "distant enemy should not be visible");

        // Bonus "Left" has [0, 1]. Territory 1 is enemy and visible.
        let left_bonus = &board.map.bonuses[0];
        let visible_enemy_count = left_bonus
            .territory_ids
            .iter()
            .filter(|&&tid| {
                let owner = state.territory_owners[tid];
                owner != 0 && owner != NEUTRAL && visible.contains(&tid)
            })
            .count();
        assert_eq!(visible_enemy_count, 1);

        // Total visible enemy territories: only territory 1.
        let total_visible_enemies = visible
            .iter()
            .filter(|&&tid| state.territory_owners[tid] == 1)
            .count();
        assert_eq!(total_visible_enemies, 1);
    }

    #[test]
    fn test_no_fog_counts_all_enemies() {
        // When fog is disabled, all enemies should be counted (no visibility filter).
        let mut map = test_map();
        map.settings.fog_of_war = false;
        let board = Board::from_map(map);
        let mut state = GameState::new(&board);
        state.territory_owners[0] = 0;
        state.territory_owners[2] = 1;
        state.territory_owners[3] = 1;

        // With fog disabled, fog_filter returns unmodified state.
        let filtered = fog_filter(&state, 0, &board);
        let enemy_total = filtered
            .territory_owners
            .iter()
            .filter(|&&o| o == 1)
            .count();
        assert_eq!(enemy_total, 2, "without fog, all enemy territories are visible");
    }
}
