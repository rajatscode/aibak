use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// An arena tournament: drop-in/drop-out play within a time window.
/// Players join when they want, play as many games as they can, and get ranked by points.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Arena {
    pub id: Uuid,
    pub name: String,
    /// Which map template to use for arena games.
    pub template: String,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    /// Per-turn time limit in seconds.
    pub time_control_secs: u32,
    pub participants: Vec<ArenaParticipant>,
    pub created_at: DateTime<Utc>,
}

/// A participant in an arena tournament.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArenaParticipant {
    pub user_id: Uuid,
    pub username: String,
    pub avatar_url: Option<String>,
    pub score: i32,
    pub games_played: u32,
    pub wins: u32,
    pub current_streak: i32,
}

/// Scoring constants for arena tournaments.
pub struct Scoring;

impl Scoring {
    /// Points awarded for a win.
    pub const WIN: i32 = 2;
    /// Points awarded to each player on a draw (timeout).
    pub const DRAW: i32 = 1;
    /// Points awarded for a loss.
    pub const LOSS: i32 = 0;

    /// Calculate total points for a win, including streak bonus.
    pub fn win_points(current_streak: i32) -> i32 {
        let streak_bonus = if current_streak > 0 {
            // +1 extra per consecutive win (the streak value is already incremented
            // before calling this, so streak=1 means first win = no bonus yet,
            // streak=2+ means consecutive wins).
            (current_streak - 1).max(0)
        } else {
            0
        };
        Self::WIN + streak_bonus
    }

    /// Berserk bonus: +1 if the player used less than half their time budget.
    pub fn berserk_bonus(time_used_secs: u32, time_budget_secs: u32) -> i32 {
        if time_budget_secs > 0 && time_used_secs < time_budget_secs / 2 {
            1
        } else {
            0
        }
    }
}

/// Status of an arena relative to the current time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArenaStatus {
    Upcoming,
    Active,
    Finished,
}

impl Arena {
    /// Determine the current status of the arena.
    pub fn status(&self) -> ArenaStatus {
        let now = Utc::now();
        if now < self.start_time {
            ArenaStatus::Upcoming
        } else if now > self.end_time {
            ArenaStatus::Finished
        } else {
            ArenaStatus::Active
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_win_points_no_streak() {
        // First win: streak=1 after increment, bonus = 0
        assert_eq!(Scoring::win_points(1), 2);
    }

    #[test]
    fn test_win_points_with_streak() {
        // Second consecutive win: streak=2, bonus = 1
        assert_eq!(Scoring::win_points(2), 3);
        // Third consecutive win: streak=3, bonus = 2
        assert_eq!(Scoring::win_points(3), 4);
    }

    #[test]
    fn test_berserk_bonus() {
        // Used 100s out of 300s budget -> less than half -> bonus
        assert_eq!(Scoring::berserk_bonus(100, 300), 1);
        // Used 200s out of 300s budget -> more than half -> no bonus
        assert_eq!(Scoring::berserk_bonus(200, 300), 0);
        // Used exactly half -> not less than half -> no bonus
        assert_eq!(Scoring::berserk_bonus(150, 300), 0);
        // Zero budget -> no bonus
        assert_eq!(Scoring::berserk_bonus(0, 0), 0);
    }
}
