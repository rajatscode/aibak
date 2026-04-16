//! Card system: reinforcement and blockade cards earned through territory captures.

use serde::{Deserialize, Serialize};

use crate::state::{GameState, NEUTRAL, PlayerId};

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Bonus, Map, MapSettings, PickingConfig, PickingMethod, Territory};

    fn test_map() -> Map {
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
                    adjacent: vec![0],
                    visual: None,
                },
            ],
            bonuses: vec![Bonus {
                id: 0,
                name: "X".into(),
                value: 2,
                territory_ids: vec![0, 1],
                visual: None,
            }],
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
    fn test_award_card_pieces_gives_card_after_3_captures() {
        let map = test_map();
        let mut state = GameState::new(&map);

        // Capture 2 territories: should get 2 pieces, no card yet.
        award_card_pieces(&mut state, 0, 2);
        assert_eq!(state.card_pieces[0], 2);
        assert!(state.hands[0].is_empty());

        // Capture 1 more: total 3 pieces -> 1 card, 0 remaining pieces.
        award_card_pieces(&mut state, 0, 1);
        assert_eq!(state.card_pieces[0], 0);
        assert_eq!(state.hands[0].len(), 1);
        assert_eq!(state.hands[0][0], Card::Reinforcement(REINFORCEMENT_VALUE));
    }

    #[test]
    fn test_award_card_pieces_multiple_cards() {
        let map = test_map();
        let mut state = GameState::new(&map);

        // Capture 7 territories at once: 7 pieces -> 2 cards, 1 leftover.
        award_card_pieces(&mut state, 0, 7);
        assert_eq!(state.card_pieces[0], 1);
        assert_eq!(state.hands[0].len(), 2);
    }

    #[test]
    fn test_blockade_multiplies_and_neutralizes() {
        let map = test_map();
        let mut state = GameState::new(&map);
        state.territory_owners[0] = 0;
        state.territory_armies[0] = 4;
        state.hands[0] = vec![Card::Blockade];

        let success = apply_blockade(&mut state, 0, 0);
        assert!(success);
        // Armies should be multiplied by 3.
        assert_eq!(state.territory_armies[0], 12);
        // Territory should become neutral.
        assert_eq!(state.territory_owners[0], NEUTRAL);
        // Blockade card should be consumed.
        assert!(state.hands[0].is_empty());
    }

    #[test]
    fn test_blockade_fails_without_card() {
        let map = test_map();
        let mut state = GameState::new(&map);
        state.territory_owners[0] = 0;
        state.territory_armies[0] = 4;
        // No blockade card in hand.

        let success = apply_blockade(&mut state, 0, 0);
        assert!(!success);
        // Nothing should change.
        assert_eq!(state.territory_armies[0], 4);
        assert_eq!(state.territory_owners[0], 0);
    }

    #[test]
    fn test_blockade_fails_on_enemy_territory() {
        let map = test_map();
        let mut state = GameState::new(&map);
        state.territory_owners[0] = 1; // enemy owns it
        state.territory_armies[0] = 4;
        state.hands[0] = vec![Card::Blockade];

        let success = apply_blockade(&mut state, 0, 0);
        assert!(!success);
    }

    #[test]
    fn test_use_reinforcement_cards() {
        let map = test_map();
        let mut state = GameState::new(&map);
        state.hands[0] = vec![
            Card::Reinforcement(5),
            Card::Blockade,
            Card::Reinforcement(5),
        ];

        // Use 1 reinforcement card.
        let bonus = use_reinforcement_cards(&mut state, 0, 1);
        assert_eq!(bonus, 5);
        // Should still have blockade + 1 reinforcement.
        assert_eq!(state.hands[0].len(), 2);
        assert!(state.hands[0].contains(&Card::Blockade));
        assert!(state.hands[0].contains(&Card::Reinforcement(5)));
    }

    #[test]
    fn test_use_reinforcement_cards_multiple() {
        let map = test_map();
        let mut state = GameState::new(&map);
        state.hands[0] = vec![
            Card::Reinforcement(5),
            Card::Reinforcement(5),
            Card::Blockade,
        ];

        let bonus = use_reinforcement_cards(&mut state, 0, 2);
        assert_eq!(bonus, 10);
        // Only blockade should remain.
        assert_eq!(state.hands[0].len(), 1);
        assert_eq!(state.hands[0][0], Card::Blockade);
    }
}
