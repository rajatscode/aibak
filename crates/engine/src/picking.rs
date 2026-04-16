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

/// Starting armies placed on each picked territory.
const STARTING_ARMIES: u32 = 5;

/// Resolve the picking phase using ABBAABBA snake draft order.
///
/// Players alternate picks in a snake pattern: A, B, B, A, A, B, B, A, ...
/// If a player runs out of submitted picks before reaching their quota,
/// they receive a random unclaimed pickable territory.
pub fn resolve_picks(
    state: &mut GameState,
    picks: [&Picks; 2],
    map: &Map,
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
            state.territory_armies[tid] = STARTING_ARMIES;
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
}
