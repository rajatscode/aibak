use serde::Serialize;

/// Rank tier definition with display metadata.
#[derive(Debug, Clone, Serialize)]
pub struct RankTier {
    pub name: &'static str,
    pub min_rp: i32,
    pub max_rp: Option<i32>,
    pub color: &'static str,
}

/// All rank tiers, ordered from lowest to highest.
pub const TIERS: &[RankTier] = &[
    RankTier { name: "bronze", min_rp: 0, max_rp: Some(99), color: "#cd7f32" },
    RankTier { name: "silver", min_rp: 100, max_rp: Some(249), color: "#c0c0c0" },
    RankTier { name: "gold", min_rp: 250, max_rp: Some(499), color: "#ffd700" },
    RankTier { name: "platinum", min_rp: 500, max_rp: Some(999), color: "#e5e4e2" },
    RankTier { name: "diamond", min_rp: 1000, max_rp: Some(1999), color: "#b9f2ff" },
    RankTier { name: "master", min_rp: 2000, max_rp: Some(3999), color: "#9b59b6" },
    RankTier { name: "grandmaster", min_rp: 4000, max_rp: None, color: "#e74c3c" },
];

/// Determine the rank tier for a given rank-point total.
pub fn rank_tier_for_rp(rp: i32) -> &'static RankTier {
    for tier in TIERS.iter().rev() {
        if rp >= tier.min_rp {
            return tier;
        }
    }
    &TIERS[0]
}

/// Calculate rank-point changes for winner and loser.
///
/// Returns `(winner_rp_gain, loser_rp_loss)` where loser_rp_loss is negative.
#[allow(dead_code)]
pub fn calculate_rp_change(winner_rp: i32, loser_rp: i32, winner_streak: i32) -> (i32, i32) {
    let winner_tier = rank_tier_for_rp(winner_rp);
    let loser_tier = rank_tier_for_rp(loser_rp);

    // Tier-difference modifier: +5 if opponent is higher tier, -5 if lower.
    let tier_diff = |my_tier: &RankTier, opp_tier: &RankTier| -> i32 {
        if opp_tier.min_rp > my_tier.min_rp {
            5
        } else if opp_tier.min_rp < my_tier.min_rp {
            -5
        } else {
            0
        }
    };

    // Winner gains.
    let base_gain: i32 = 25;
    let streak_bonus = (winner_streak.max(0) * 5).min(25);
    let win_modifier = tier_diff(winner_tier, loser_tier);
    let gain = (base_gain + streak_bonus + win_modifier).max(5); // minimum +5

    // Loser loses.
    let base_loss: i32 = -20;
    let loss_modifier = -tier_diff(loser_tier, winner_tier);
    let loss = (base_loss + loss_modifier).min(-5); // minimum -5 loss

    (gain, loss)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rank_tier_for_rp() {
        assert_eq!(rank_tier_for_rp(0).name, "bronze");
        assert_eq!(rank_tier_for_rp(99).name, "bronze");
        assert_eq!(rank_tier_for_rp(100).name, "silver");
        assert_eq!(rank_tier_for_rp(250).name, "gold");
        assert_eq!(rank_tier_for_rp(4000).name, "grandmaster");
        assert_eq!(rank_tier_for_rp(10000).name, "grandmaster");
    }

    #[test]
    fn test_calculate_rp_change_equal_tier() {
        let (gain, loss) = calculate_rp_change(50, 50, 0);
        assert_eq!(gain, 25);
        assert_eq!(loss, -20);
    }

    #[test]
    fn test_calculate_rp_change_streak_bonus() {
        let (gain, _) = calculate_rp_change(50, 50, 3);
        assert_eq!(gain, 25 + 15); // base + 3*5 streak
    }

    #[test]
    fn test_calculate_rp_change_streak_cap() {
        let (gain, _) = calculate_rp_change(50, 50, 10);
        assert_eq!(gain, 25 + 25); // streak capped at 25
    }

    #[test]
    fn test_calculate_rp_change_higher_opponent() {
        // Winner is bronze (50 RP), loser is silver (150 RP).
        let (gain, loss) = calculate_rp_change(50, 150, 0);
        assert_eq!(gain, 30); // 25 + 5 (opponent higher)
        assert_eq!(loss, -15); // -20 + 5 (opponent lower)
    }

    #[test]
    fn test_rp_cant_go_below_minimum() {
        // Even with disadvantageous matchup, minimum gain is 5.
        let (gain, loss) = calculate_rp_change(4000, 50, 0);
        assert!(gain >= 5);
        assert!(loss <= -5);
    }
}
