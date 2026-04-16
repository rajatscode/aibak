use rand::seq::SliceRandom;
use rand::Rng;

use crate::map::Map;
use crate::state::{GameState, Phase, NEUTRAL};

/// Generate Random Warlords pick options.
/// Selects exactly one random territory from each bonus that has value > 0.
/// This is the standard competitive picking mechanic.
pub fn generate_pick_options(map: &Map, rng: &mut impl Rng) -> Vec<usize> {
    let mut options = Vec::new();

    for bonus in &map.bonuses {
        if bonus.value == 0 {
            continue;
        }
        // Collect non-wasteland territories in this bonus
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

    // Shuffle the final list so bonus order doesn't leak info
    options.shuffle(rng);
    options
}

/// Represents a player's territory picks, ordered by priority.
pub type Picks = Vec<usize>;

/// Starting armies placed on each picked territory.
const STARTING_ARMIES: u32 = 5;

/// Resolve the picking phase. Each player submits ordered picks.
/// Picks are resolved alternating: P0 pick 1, P1 pick 1, P0 pick 2, P1 pick 2, etc.
/// If both players pick the same territory, the first in order gets it;
/// the other player falls through to their next priority pick.
pub fn resolve_picks(
    state: &mut GameState,
    picks: [&Picks; 2],
    map: &Map,
) {
    let num_picks = map.picking.num_picks;
    let mut claimed: Vec<bool> = vec![false; map.territory_count()];
    let mut player_assigned: [Vec<usize>; 2] = [Vec::new(), Vec::new()];

    // Interleave picks: seat 0 first, then seat 1, alternating.
    for _round in 0..num_picks {
        for seat in 0..2usize {
            if player_assigned[seat].len() >= num_picks {
                continue;
            }
            // Find this player's next unclaimed pick.
            for &tid in picks[seat].iter() {
                if !claimed[tid] && player_assigned[seat].len() < num_picks {
                    claimed[tid] = true;
                    player_assigned[seat].push(tid);
                    break;
                }
            }
        }
    }

    // Assign territories with starting armies.
    for seat in 0..2u8 {
        for &tid in &player_assigned[seat as usize] {
            state.territory_owners[tid] = seat;
            state.territory_armies[tid] = STARTING_ARMIES;
        }
    }

    // Neutral territories keep their default armies.
    for tid in 0..map.territory_count() {
        if state.territory_owners[tid] == NEUTRAL && !map.territories[tid].is_wasteland {
            state.territory_armies[tid] = map.territories[tid].default_armies;
        }
    }

    state.phase = Phase::Play;
    state.turn = 1;
}
