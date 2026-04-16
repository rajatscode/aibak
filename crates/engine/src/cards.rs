use serde::{Deserialize, Serialize};

use crate::state::{GameState, PlayerId, NEUTRAL};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Card {
    /// Add extra armies to deploy this turn.
    Reinforcement(u32),
    /// Multiply armies on a territory by a factor, then the territory becomes neutral.
    Blockade,
}

/// Blockade multiplier.
const BLOCKADE_MULTIPLIER: u32 = 3;

/// Card pieces needed to earn a reinforcement card.
pub const PIECES_PER_CARD: u32 = 3;

/// Default reinforcement card value.
pub const REINFORCEMENT_VALUE: u32 = 5;

/// Award card pieces for territories captured this turn and convert to cards.
pub fn award_card_pieces(state: &mut GameState, player: PlayerId, territories_captured: u32) {
    let idx = player as usize;
    state.card_pieces[idx] += territories_captured;
    while state.card_pieces[idx] >= PIECES_PER_CARD {
        state.card_pieces[idx] -= PIECES_PER_CARD;
        state.hands[idx].push(Card::Reinforcement(REINFORCEMENT_VALUE));
    }
}

/// Apply a blockade card to a territory.
pub fn apply_blockade(state: &mut GameState, player: PlayerId, territory: usize) -> bool {
    if state.territory_owners[territory] != player {
        return false;
    }
    // Remove the blockade card from hand.
    let hand = &mut state.hands[player as usize];
    if let Some(pos) = hand.iter().position(|c| *c == Card::Blockade) {
        hand.remove(pos);
    } else {
        return false;
    }
    state.territory_armies[territory] *= BLOCKADE_MULTIPLIER;
    state.territory_owners[territory] = NEUTRAL;
    true
}

/// Get extra deploy armies from reinforcement cards in a player's orders.
/// Removes used reinforcement cards from hand. Returns total bonus armies.
pub fn use_reinforcement_cards(state: &mut GameState, player: PlayerId, count: usize) -> u32 {
    let hand = &mut state.hands[player as usize];
    let mut total = 0u32;
    let mut used = 0;
    hand.retain(|card| {
        if used >= count {
            return true;
        }
        if let Card::Reinforcement(value) = card {
            total += value;
            used += 1;
            false
        } else {
            true
        }
    });
    total
}
