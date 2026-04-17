//! Core game state: territory ownership, armies, turn tracking, and elimination logic.

use serde::{Deserialize, Serialize};

use crate::board::Board;
use crate::cards::Card;

/// Player seat index: 0 or 1.
pub type PlayerId = u8;

/// Neutral owner sentinel.
pub const NEUTRAL: PlayerId = 255;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    Picking,
    Play,
    Finished,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameState {
    pub territory_owners: Vec<PlayerId>,
    pub territory_armies: Vec<u32>,
    pub turn: u32,
    pub phase: Phase,
    /// Cards in each player's hand.
    pub hands: [Vec<Card>; 2],
    /// Card pieces earned (3 pieces = 1 reinforcement card).
    pub card_pieces: [u32; 2],
    /// Which players are still alive.
    pub alive: [bool; 2],
    /// Winner, if any.
    pub winner: Option<PlayerId>,
}

impl GameState {
    /// Create initial state from a board. All territories are neutral with default armies.
    pub fn new(board: &Board) -> Self {
        let n = board.map.territory_count();
        let mut territory_armies = Vec::with_capacity(n);
        for t in &board.map.territories {
            if t.is_wasteland {
                territory_armies.push(board.settings().wasteland_armies);
            } else {
                territory_armies.push(t.default_armies);
            }
        }

        Self {
            territory_owners: vec![NEUTRAL; n],
            territory_armies,
            turn: 0,
            phase: Phase::Picking,
            hands: [Vec::new(), Vec::new()],
            card_pieces: [0; 2],
            alive: [true; 2],
            winner: None,
        }
    }

    /// Calculate income for a player: base + sum of completed bonuses.
    pub fn income(&self, player: PlayerId, board: &Board) -> u32 {
        let mut income = board.settings().base_income;
        for bonus in &board.map.bonuses {
            let owns_all = bonus
                .territory_ids
                .iter()
                .all(|&tid| self.territory_owners[tid] == player);
            if owns_all {
                income += bonus.value;
            }
        }
        income
    }

    /// Count territories owned by a player.
    pub fn territory_count_for(&self, player: PlayerId) -> usize {
        self.territory_owners
            .iter()
            .filter(|&&o| o == player)
            .count()
    }

    /// Check if a player has been eliminated and update state accordingly.
    pub fn check_elimination(&mut self) {
        for p in 0..2u8 {
            if self.alive[p as usize] && self.territory_count_for(p) == 0 {
                self.alive[p as usize] = false;
                let opponent = 1 - p;
                if self.alive[opponent as usize] {
                    self.winner = Some(opponent);
                    self.phase = Phase::Finished;
                }
            }
        }
        // Handle simultaneous elimination: both players lost all territories.
        if !self.alive[0] && !self.alive[1] && self.phase != Phase::Finished {
            self.phase = Phase::Finished;
        }
    }
}
