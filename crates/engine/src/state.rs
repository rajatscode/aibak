//! Core game state: territory ownership, armies, turn tracking, and elimination logic.

use serde::{Deserialize, Serialize};

use crate::board::Board;
use crate::cards::Card;

/// Player seat index: 0 or 1.
pub type PlayerId = u8;

fn default_alive() -> [bool; 2] {
    [true; 2]
}

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
    #[serde(default = "default_alive")]
    pub alive: [bool; 2],
    /// Winner, if any.
    #[serde(default)]
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
        // Collect eliminations first to handle simultaneous elimination fairly.
        for p in 0..2u8 {
            if self.alive[p as usize] && self.territory_count_for(p) == 0 {
                self.alive[p as usize] = false;
            }
        }
        // Resolve after checking both players.
        match (self.alive[0], self.alive[1]) {
            (false, false) => {
                // Simultaneous elimination: draw (no winner).
                self.phase = Phase::Finished;
            }
            (true, false) => {
                self.winner = Some(0);
                self.phase = Phase::Finished;
            }
            (false, true) => {
                self.winner = Some(1);
                self.phase = Phase::Finished;
            }
            _ => {} // Both still alive.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::Board;
    use crate::map::{Bonus, MapFile, MapSettings, PickingConfig, PickingMethod, Territory};

    fn test_board() -> Board {
        Board::from_map(MapFile {
            id: "test".into(),
            name: "Test".into(),
            territories: vec![
                Territory { id: 0, name: "A".into(), bonus_id: 0, is_wasteland: false, default_armies: 2, adjacent: vec![1], visual: None },
                Territory { id: 1, name: "B".into(), bonus_id: 0, is_wasteland: false, default_armies: 2, adjacent: vec![0], visual: None },
            ],
            bonuses: vec![Bonus { id: 0, name: "X".into(), value: 2, territory_ids: vec![0, 1], visual: None }],
            picking: PickingConfig { num_picks: 1, method: PickingMethod::RandomWarlords },
            settings: MapSettings { luck_pct: 0, base_income: 5, wasteland_armies: 10, unpicked_neutral_armies: 4, fog_of_war: false, offense_kill_rate: 0.6, defense_kill_rate: 0.7 },
        })
    }

    #[test]
    fn test_simultaneous_elimination_is_draw() {
        let board = test_board();
        let mut state = GameState::new(&board);
        state.territory_owners = vec![0, 1];
        state.phase = Phase::Play;
        state.alive = [true, true];

        // Both players lose all territories simultaneously.
        state.territory_owners = vec![NEUTRAL, NEUTRAL];
        state.check_elimination();

        assert!(!state.alive[0]);
        assert!(!state.alive[1]);
        assert_eq!(state.phase, Phase::Finished);
        assert_eq!(state.winner, None, "simultaneous elimination should be a draw");
    }

    #[test]
    fn test_single_elimination_sets_winner() {
        let board = test_board();
        let mut state = GameState::new(&board);
        state.territory_owners = vec![0, 0]; // player 0 owns both
        state.phase = Phase::Play;
        state.alive = [true, true];

        state.check_elimination();

        assert!(state.alive[0]);
        assert!(!state.alive[1]);
        assert_eq!(state.winner, Some(0));
        assert_eq!(state.phase, Phase::Finished);
    }
}
