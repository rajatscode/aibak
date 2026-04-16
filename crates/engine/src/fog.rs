//! Fog of war: visibility computation and state filtering per player.

use std::collections::HashSet;

use crate::map::Map;
use crate::state::{GameState, NEUTRAL, PlayerId};

/// Compute the set of territory IDs visible to a player.
/// A territory is visible if:
/// - Owned by the player, OR
/// - Adjacent to any territory owned by the player.
pub fn visible_territories(state: &GameState, player: PlayerId, map: &Map) -> HashSet<usize> {
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

/// Create a fog-filtered copy of the game state for a player.
/// Non-visible territories show as neutral with unknown army counts.
pub fn fog_filter(state: &GameState, player: PlayerId, map: &Map) -> GameState {
    if !map.settings.fog_of_war {
        return state.clone();
    }

    let visible = visible_territories(state, player, map);
    let mut filtered = state.clone();

    for tid in 0..map.territory_count() {
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
    use crate::map::{Bonus, MapSettings, PickingConfig, PickingMethod, Territory};

    fn test_map() -> Map {
        // Simple 4-territory map: 0-1-2-3 in a line.
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
    fn test_visibility() {
        let map = test_map();
        let mut state = GameState::new(&map);
        state.territory_owners[0] = 0;
        state.territory_owners[3] = 1;

        let vis0 = visible_territories(&state, 0, &map);
        assert!(vis0.contains(&0)); // owns it
        assert!(vis0.contains(&1)); // adjacent
        assert!(!vis0.contains(&2)); // not adjacent
        assert!(!vis0.contains(&3)); // not adjacent

        let vis1 = visible_territories(&state, 1, &map);
        assert!(vis1.contains(&3));
        assert!(vis1.contains(&2));
        assert!(!vis1.contains(&1));
        assert!(!vis1.contains(&0));
    }

    #[test]
    fn test_fog_filter_hides_enemy() {
        let map = test_map();
        let mut state = GameState::new(&map);
        state.territory_owners[0] = 0;
        state.territory_armies[0] = 5;
        state.territory_owners[3] = 1;
        state.territory_armies[3] = 8;

        let filtered = fog_filter(&state, 0, &map);
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
        let mut state = GameState::new(&map);
        // Player 0 owns territories 0 and 3 (opposite ends of the line).
        state.territory_owners[0] = 0;
        state.territory_owners[3] = 0;

        let vis = visible_territories(&state, 0, &map);
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
        let mut state = GameState::new(&map);
        state.territory_owners[0] = 0;
        state.territory_owners[3] = 1;
        // Give opponent cards and card pieces.
        state.hands[1] = vec![Card::Reinforcement(5), Card::Blockade];
        state.card_pieces[1] = 2;

        let filtered = fog_filter(&state, 0, &map);
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
        let map = Map::load(&maps_dir.join("small_earth.json")).expect("load map");

        let mut state = GameState::new(&map);
        // Player 0 owns Alaska (0) and NW Territory (1).
        state.territory_owners[0] = 0;
        state.territory_owners[1] = 0;

        let vis = visible_territories(&state, 0, &map);
        // Should see owned territories.
        assert!(vis.contains(&0));
        assert!(vis.contains(&1));
        // Should see all neighbors of both.
        for &owned in &[0, 1] {
            for &adj in &map.territories[owned].adjacent {
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
}
