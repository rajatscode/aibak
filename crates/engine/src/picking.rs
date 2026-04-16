use rand::seq::SliceRandom;
use rand::Rng;

use crate::map::Map;
use crate::state::{GameState, Phase, NEUTRAL};

/// Generate Random Warlords pick options.
/// Selects exactly one random territory from each bonus that has value > 0.
pub fn generate_pick_options(map: &Map, rng: &mut impl Rng) -> Vec<usize> {
    let mut options = Vec::new();

    for bonus in &map.bonuses {
        if bonus.value == 0 {
            continue;
        }
        let mut candidates: Vec<usize> = bonus
            .territory_ids
            .iter()
            .copied()
            .filter(|&tid| !map.territories[tid].is_wasteland)
            .collect();
        if candidates.is_empty() {
            continue;
        }
        candidates.shuffle(rng);
        options.push(candidates[0]);
    }

    options.shuffle(rng);
    options
}

/// Represents a player's territory picks, ordered by priority.
pub type Picks = Vec<usize>;

/// Default starting armies placed on each picked territory.
pub const DEFAULT_STARTING_ARMIES: u32 = 5;

/// Resolve the picking phase using ABBAABBA snake draft order.
///
/// Players alternate picks in a snake pattern: A, B, B, A, A, B, B, A, ...
/// If a player runs out of submitted picks before reaching their quota,
/// they receive a random unclaimed pickable territory.
///
/// `starting_armies` controls how many armies are placed on each picked territory.
pub fn resolve_picks(
    state: &mut GameState,
    picks: [&Picks; 2],
    map: &Map,
    starting_armies: u32,
) {
    let num_picks = map.picking.num_picks;
    let mut claimed: Vec<bool> = vec![false; map.territory_count()];
    let mut player_assigned: [Vec<usize>; 2] = [Vec::new(), Vec::new()];
    let mut pick_indices: [usize; 2] = [0, 0]; // track position in each player's pick list

    // Generate the ABBAABBA snake draft order
    let total_picks = num_picks * 2;
    let draft_order = snake_draft_order(total_picks);

    // All pickable territory IDs (for random fallback)
    let all_pickable: Vec<usize> = (0..map.territory_count())
        .filter(|&tid| !map.territories[tid].is_wasteland)
        .collect();

    let mut rng = rand::thread_rng();

    for &seat in &draft_order {
        if player_assigned[seat].len() >= num_picks {
            continue;
        }

        // Try to find this player's next unclaimed pick from their submitted list
        let mut found = false;
        while pick_indices[seat] < picks[seat].len() {
            let tid = picks[seat][pick_indices[seat]];
            pick_indices[seat] += 1;
            if !claimed[tid] && tid < map.territory_count() {
                claimed[tid] = true;
                player_assigned[seat].push(tid);
                found = true;
                break;
            }
        }

        // If player ran out of picks, assign a random unclaimed territory
        if !found {
            let mut unclaimed: Vec<usize> = all_pickable
                .iter()
                .copied()
                .filter(|&tid| !claimed[tid])
                .collect();
            unclaimed.shuffle(&mut rng);
            if let Some(tid) = unclaimed.first() {
                claimed[*tid] = true;
                player_assigned[seat].push(*tid);
            }
        }
    }

    // Assign territories with starting armies
    for seat in 0..2u8 {
        for &tid in &player_assigned[seat as usize] {
            state.territory_owners[tid] = seat;
            state.territory_armies[tid] = starting_armies;
        }
    }

    // Neutral territories keep their default armies
    for tid in 0..map.territory_count() {
        if state.territory_owners[tid] == NEUTRAL && !map.territories[tid].is_wasteland {
            state.territory_armies[tid] = map.territories[tid].default_armies;
        }
    }

    state.phase = Phase::Play;
    state.turn = 1;
}

/// Generate ABBAABBA snake draft order for N total picks.
/// Returns a vec of seat indices (0 or 1).
fn snake_draft_order(total: usize) -> Vec<usize> {
    let mut order = Vec::with_capacity(total);
    // Pattern repeats in groups of 4: A, B, B, A
    for i in 0..total {
        let pos_in_cycle = i % 4;
        let seat = match pos_in_cycle {
            0 => 0, // A
            1 => 1, // B
            2 => 1, // B
            3 => 0, // A
            _ => unreachable!(),
        };
        order.push(seat);
    }
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snake_draft_order() {
        let order = snake_draft_order(8);
        assert_eq!(order, vec![0, 1, 1, 0, 0, 1, 1, 0]);
    }

    #[test]
    fn test_snake_draft_order_odd() {
        let order = snake_draft_order(5);
        assert_eq!(order, vec![0, 1, 1, 0, 0]);
    }

    #[test]
    fn test_snake_draft_order_various_sizes() {
        // 0 picks
        assert_eq!(snake_draft_order(0), Vec::<usize>::new());
        // 1 pick: just A
        assert_eq!(snake_draft_order(1), vec![0]);
        // 2 picks: A, B
        assert_eq!(snake_draft_order(2), vec![0, 1]);
        // 3 picks: A, B, B
        assert_eq!(snake_draft_order(3), vec![0, 1, 1]);
        // 4 picks: A, B, B, A
        assert_eq!(snake_draft_order(4), vec![0, 1, 1, 0]);
        // 12 picks: 3 full cycles
        let order = snake_draft_order(12);
        assert_eq!(order, vec![0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0]);
        // Each player gets exactly 6 picks
        assert_eq!(order.iter().filter(|&&s| s == 0).count(), 6);
        assert_eq!(order.iter().filter(|&&s| s == 1).count(), 6);
    }

    #[test]
    fn test_resolve_picks_contested() {
        use crate::map::{Bonus, MapSettings, PickingConfig, PickingMethod, Territory};
        // Map with 6 territories, 2 bonuses, num_picks=2.
        let map = Map {
            id: "contest".into(),
            name: "Contest".into(),
            territories: (0..6)
                .map(|i| Territory {
                    id: i,
                    name: format!("T{}", i),
                    bonus_id: i / 3,
                    is_wasteland: false,
                    default_armies: 2,
                    adjacent: if i == 0 { vec![1] }
                        else if i == 5 { vec![4] }
                        else { vec![i - 1, i + 1] },
                    visual: None,
                })
                .collect(),
            bonuses: vec![
                Bonus { id: 0, name: "A".into(), value: 3, territory_ids: vec![0, 1, 2], visual: None },
                Bonus { id: 1, name: "B".into(), value: 3, territory_ids: vec![3, 4, 5], visual: None },
            ],
            picking: PickingConfig { num_picks: 2, method: PickingMethod::RandomWarlords },
            settings: MapSettings {
                luck_pct: 0, base_income: 5, wasteland_armies: 10,
                unpicked_neutral_armies: 4, fog_of_war: true,
                offense_kill_rate: 0.6, defense_kill_rate: 0.7,
            },
        };

        let mut state = GameState::new(&map);
        // Both players submit identical pick lists.
        let picks = vec![0, 1, 2, 3, 4, 5];
        resolve_picks(&mut state, [&picks, &picks], &map, DEFAULT_STARTING_ARMIES);

        // Each player should get exactly 2 territories.
        assert_eq!(state.territory_count_for(0), 2);
        assert_eq!(state.territory_count_for(1), 2);
        // No territory should be owned by both.
        for tid in 0..6 {
            let owner = state.territory_owners[tid];
            if owner != NEUTRAL {
                assert!(owner == 0 || owner == 1);
            }
        }
    }

    #[test]
    fn test_all_assigned_get_starting_armies() {
        use crate::map::{Bonus, MapSettings, PickingConfig, PickingMethod, Territory};
        let map = Map {
            id: "test".into(),
            name: "Test".into(),
            territories: (0..8)
                .map(|i| Territory {
                    id: i,
                    name: format!("T{}", i),
                    bonus_id: i / 4,
                    is_wasteland: false,
                    default_armies: 2,
                    adjacent: if i == 0 { vec![1] }
                        else if i == 7 { vec![6] }
                        else { vec![i - 1, i + 1] },
                    visual: None,
                })
                .collect(),
            bonuses: vec![
                Bonus { id: 0, name: "A".into(), value: 3, territory_ids: vec![0, 1, 2, 3], visual: None },
                Bonus { id: 1, name: "B".into(), value: 3, territory_ids: vec![4, 5, 6, 7], visual: None },
            ],
            picking: PickingConfig { num_picks: 3, method: PickingMethod::RandomWarlords },
            settings: MapSettings {
                luck_pct: 0, base_income: 5, wasteland_armies: 10,
                unpicked_neutral_armies: 4, fog_of_war: true,
                offense_kill_rate: 0.6, defense_kill_rate: 0.7,
            },
        };

        let mut state = GameState::new(&map);
        let picks_a = vec![0, 1, 2];
        let picks_b = vec![4, 5, 6];
        resolve_picks(&mut state, [&picks_a, &picks_b], &map, DEFAULT_STARTING_ARMIES);

        // Every assigned territory should have exactly DEFAULT_STARTING_ARMIES (5).
        for tid in 0..8 {
            if state.territory_owners[tid] != NEUTRAL {
                assert_eq!(
                    state.territory_armies[tid], DEFAULT_STARTING_ARMIES,
                    "Territory {} owned by {} should have {} armies, got {}",
                    tid, state.territory_owners[tid], DEFAULT_STARTING_ARMIES, state.territory_armies[tid]
                );
            }
        }
    }

    #[test]
    fn test_random_fallback_for_insufficient_picks() {
        use crate::map::{Bonus, MapSettings, PickingConfig, PickingMethod, Territory};
        let map = Map {
            id: "fallback".into(),
            name: "Fallback".into(),
            territories: (0..6)
                .map(|i| Territory {
                    id: i,
                    name: format!("T{}", i),
                    bonus_id: i / 3,
                    is_wasteland: false,
                    default_armies: 2,
                    adjacent: if i == 0 { vec![1] }
                        else if i == 5 { vec![4] }
                        else { vec![i - 1, i + 1] },
                    visual: None,
                })
                .collect(),
            bonuses: vec![
                Bonus { id: 0, name: "A".into(), value: 3, territory_ids: vec![0, 1, 2], visual: None },
                Bonus { id: 1, name: "B".into(), value: 3, territory_ids: vec![3, 4, 5], visual: None },
            ],
            picking: PickingConfig { num_picks: 2, method: PickingMethod::RandomWarlords },
            settings: MapSettings {
                luck_pct: 0, base_income: 5, wasteland_armies: 10,
                unpicked_neutral_armies: 4, fog_of_war: true,
                offense_kill_rate: 0.6, defense_kill_rate: 0.7,
            },
        };

        let mut state = GameState::new(&map);
        // Player A submits only 1 pick, player B submits 0 picks.
        let picks_a: Vec<usize> = vec![0];
        let picks_b: Vec<usize> = vec![];
        resolve_picks(&mut state, [&picks_a, &picks_b], &map, DEFAULT_STARTING_ARMIES);

        // Both players should still get their quota via random fallback.
        assert_eq!(state.territory_count_for(0), 2);
        assert_eq!(state.territory_count_for(1), 2);
        assert_eq!(state.phase, Phase::Play);
    }
}
