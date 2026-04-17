use serde::Serialize;

/// Unique identifier for each achievement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AchievementId {
    FirstBlood,
    Veteran,
    Commander,
    Conqueror,
    FlawlessVictory,
    Underdog,
    Speedrun,
    Marathon,
    BonusHunter,
    HatTrick,
    Unstoppable,
    SilverLeague,
    GoldLeague,
    DiamondLeague,
    Explorer,
}

/// Static definition of an achievement.
#[derive(Debug, Clone, Serialize)]
pub struct AchievementDef {
    pub id: AchievementId,
    pub name: &'static str,
    pub description: &'static str,
    pub icon: &'static str,
    pub category: &'static str,
}

/// An earned achievement with the game number at which it was unlocked.
#[derive(Debug, Clone, Serialize)]
pub struct EarnedAchievement {
    pub id: AchievementId,
    pub game_number: u32,
}

/// Full achievement status returned by the API.
#[derive(Debug, Clone, Serialize)]
pub struct AchievementView {
    pub id: AchievementId,
    pub name: &'static str,
    pub description: &'static str,
    pub icon: &'static str,
    pub category: &'static str,
    pub earned: bool,
    /// Game number at which it was earned, if any.
    pub earned_at: Option<u32>,
}

/// All achievement definitions.
pub const ACHIEVEMENTS: &[AchievementDef] = &[
    // Win-based
    AchievementDef {
        id: AchievementId::FirstBlood,
        name: "First Blood",
        description: "Win your first game",
        icon: "\u{2694}\u{fe0f}",
        category: "Wins",
    },
    AchievementDef {
        id: AchievementId::Veteran,
        name: "Veteran",
        description: "Win 10 games",
        icon: "\u{2b50}",
        category: "Wins",
    },
    AchievementDef {
        id: AchievementId::Commander,
        name: "Commander",
        description: "Win 25 games",
        icon: "\u{1f396}\u{fe0f}",
        category: "Wins",
    },
    AchievementDef {
        id: AchievementId::Conqueror,
        name: "Conqueror",
        description: "Win 50 games",
        icon: "\u{1f451}",
        category: "Wins",
    },
    // Skill-based
    AchievementDef {
        id: AchievementId::FlawlessVictory,
        name: "Flawless Victory",
        description: "Win without losing any starting territories",
        icon: "\u{1f4a0}",
        category: "Skill",
    },
    AchievementDef {
        id: AchievementId::Underdog,
        name: "Underdog",
        description: "Win a game where your win probability was below 20%",
        icon: "\u{1f43a}",
        category: "Skill",
    },
    AchievementDef {
        id: AchievementId::Speedrun,
        name: "Speedrun",
        description: "Win a game in under 8 turns",
        icon: "\u{26a1}",
        category: "Skill",
    },
    AchievementDef {
        id: AchievementId::Marathon,
        name: "Marathon",
        description: "Win a game that lasted over 30 turns",
        icon: "\u{1f3c3}",
        category: "Skill",
    },
    AchievementDef {
        id: AchievementId::BonusHunter,
        name: "Bonus Hunter",
        description: "Control 3 complete bonuses simultaneously",
        icon: "\u{1f3af}",
        category: "Skill",
    },
    // Streak-based
    AchievementDef {
        id: AchievementId::HatTrick,
        name: "Hat Trick",
        description: "Win 3 games in a row",
        icon: "\u{1f3a9}",
        category: "Streak",
    },
    AchievementDef {
        id: AchievementId::Unstoppable,
        name: "Unstoppable",
        description: "Win 5 games in a row",
        icon: "\u{1f525}",
        category: "Streak",
    },
    // Rating-based
    AchievementDef {
        id: AchievementId::SilverLeague,
        name: "Silver League",
        description: "Reach Silver rank",
        icon: "\u{1fa99}",
        category: "Rating",
    },
    AchievementDef {
        id: AchievementId::GoldLeague,
        name: "Gold League",
        description: "Reach Gold rank",
        icon: "\u{1f947}",
        category: "Rating",
    },
    AchievementDef {
        id: AchievementId::DiamondLeague,
        name: "Diamond League",
        description: "Reach Diamond rank",
        icon: "\u{1f48e}",
        category: "Rating",
    },
    // Special
    AchievementDef {
        id: AchievementId::Explorer,
        name: "Explorer",
        description: "Play a game on every available map",
        icon: "\u{1f5fa}\u{fe0f}",
        category: "Special",
    },
];

/// Context passed to the achievement checker after a game ends.
pub struct GameContext {
    pub won: bool,
    pub total_wins: u32,
    pub turn_count: u32,
    pub streak: i32,
    pub start_win_prob: f64,
    pub rating: f64,
    /// Whether the player still owns all territories they picked at game start.
    pub kept_all_starting_territories: bool,
    /// Maximum number of complete bonuses owned simultaneously during the game.
    pub max_simultaneous_bonuses: u32,
    /// Set of distinct map names played across all games.
    pub maps_played: Vec<String>,
    /// Total number of built-in maps available.
    pub total_maps_available: u32,
}

/// Check all achievements and return newly earned ones.
pub fn check_achievements(
    earned: &[EarnedAchievement],
    ctx: &GameContext,
    game_number: u32,
) -> Vec<EarnedAchievement> {
    let mut newly_earned = Vec::new();

    let already_has = |id: AchievementId| -> bool { earned.iter().any(|e| e.id == id) };

    // Win-based (only check if the player won)
    if ctx.won {
        if !already_has(AchievementId::FirstBlood) && ctx.total_wins >= 1 {
            newly_earned.push(EarnedAchievement {
                id: AchievementId::FirstBlood,
                game_number,
            });
        }
        if !already_has(AchievementId::Veteran) && ctx.total_wins >= 10 {
            newly_earned.push(EarnedAchievement {
                id: AchievementId::Veteran,
                game_number,
            });
        }
        if !already_has(AchievementId::Commander) && ctx.total_wins >= 25 {
            newly_earned.push(EarnedAchievement {
                id: AchievementId::Commander,
                game_number,
            });
        }
        if !already_has(AchievementId::Conqueror) && ctx.total_wins >= 50 {
            newly_earned.push(EarnedAchievement {
                id: AchievementId::Conqueror,
                game_number,
            });
        }

        // Skill-based (win required)
        if !already_has(AchievementId::FlawlessVictory) && ctx.kept_all_starting_territories {
            newly_earned.push(EarnedAchievement {
                id: AchievementId::FlawlessVictory,
                game_number,
            });
        }
        if !already_has(AchievementId::Underdog) && ctx.start_win_prob < 0.20 {
            newly_earned.push(EarnedAchievement {
                id: AchievementId::Underdog,
                game_number,
            });
        }
        if !already_has(AchievementId::Speedrun) && ctx.turn_count < 8 {
            newly_earned.push(EarnedAchievement {
                id: AchievementId::Speedrun,
                game_number,
            });
        }
        if !already_has(AchievementId::Marathon) && ctx.turn_count > 30 {
            newly_earned.push(EarnedAchievement {
                id: AchievementId::Marathon,
                game_number,
            });
        }
    }

    // Bonus Hunter (can earn even if you lose, just need 3 simultaneous)
    if !already_has(AchievementId::BonusHunter) && ctx.max_simultaneous_bonuses >= 3 {
        newly_earned.push(EarnedAchievement {
            id: AchievementId::BonusHunter,
            game_number,
        });
    }

    // Streak-based
    if !already_has(AchievementId::HatTrick) && ctx.streak >= 3 {
        newly_earned.push(EarnedAchievement {
            id: AchievementId::HatTrick,
            game_number,
        });
    }
    if !already_has(AchievementId::Unstoppable) && ctx.streak >= 5 {
        newly_earned.push(EarnedAchievement {
            id: AchievementId::Unstoppable,
            game_number,
        });
    }

    // Rating-based (Glicko rating thresholds: Silver 1200+, Gold 1400+, Diamond 1800+)
    if !already_has(AchievementId::SilverLeague) && ctx.rating >= 1200.0 {
        newly_earned.push(EarnedAchievement {
            id: AchievementId::SilverLeague,
            game_number,
        });
    }
    if !already_has(AchievementId::GoldLeague) && ctx.rating >= 1400.0 {
        newly_earned.push(EarnedAchievement {
            id: AchievementId::GoldLeague,
            game_number,
        });
    }
    if !already_has(AchievementId::DiamondLeague) && ctx.rating >= 1800.0 {
        newly_earned.push(EarnedAchievement {
            id: AchievementId::DiamondLeague,
            game_number,
        });
    }

    // Explorer: played on every available map
    if !already_has(AchievementId::Explorer)
        && ctx.total_maps_available > 0
        && ctx.maps_played.len() as u32 >= ctx.total_maps_available
    {
        newly_earned.push(EarnedAchievement {
            id: AchievementId::Explorer,
            game_number,
        });
    }

    newly_earned
}

/// Build the full achievement list with earned status for API response.
pub fn build_achievement_views(earned: &[EarnedAchievement]) -> Vec<AchievementView> {
    ACHIEVEMENTS
        .iter()
        .map(|def| {
            let earned_entry = earned.iter().find(|e| e.id == def.id);
            AchievementView {
                id: def.id,
                name: def.name,
                description: def.description,
                icon: def.icon,
                category: def.category,
                earned: earned_entry.is_some(),
                earned_at: earned_entry.map(|e| e.game_number),
            }
        })
        .collect()
}

/// Build achievement views for only the newly earned achievements (used for turn-end toasts).
pub fn build_newly_earned_views(earned: &[EarnedAchievement]) -> Vec<AchievementView> {
    earned
        .iter()
        .map(|e| {
            let def = ACHIEVEMENTS.iter().find(|a| a.id == e.id).expect("Invalid achievement ID");
            AchievementView {
                id: def.id,
                name: def.name,
                description: def.description,
                icon: def.icon,
                category: def.category,
                earned: true,
                earned_at: Some(e.game_number),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_context() -> GameContext {
        GameContext {
            won: true,
            total_wins: 1,
            turn_count: 15,
            streak: 1,
            start_win_prob: 0.5,
            rating: 1500.0,
            kept_all_starting_territories: false,
            max_simultaneous_bonuses: 0,
            maps_played: vec!["small_earth".to_string()],
            total_maps_available: 2,
        }
    }

    #[test]
    fn test_first_blood() {
        let ctx = base_context();
        let earned = check_achievements(&[], &ctx, 1);
        assert!(earned.iter().any(|e| e.id == AchievementId::FirstBlood));
    }

    #[test]
    fn test_no_duplicate() {
        let ctx = base_context();
        let existing = vec![EarnedAchievement {
            id: AchievementId::FirstBlood,
            game_number: 1,
        }];
        let earned = check_achievements(&existing, &ctx, 2);
        assert!(!earned.iter().any(|e| e.id == AchievementId::FirstBlood));
    }

    #[test]
    fn test_underdog() {
        let mut ctx = base_context();
        ctx.start_win_prob = 0.15;
        let earned = check_achievements(&[], &ctx, 1);
        assert!(earned.iter().any(|e| e.id == AchievementId::Underdog));
    }

    #[test]
    fn test_speedrun() {
        let mut ctx = base_context();
        ctx.turn_count = 5;
        let earned = check_achievements(&[], &ctx, 1);
        assert!(earned.iter().any(|e| e.id == AchievementId::Speedrun));
    }

    #[test]
    fn test_bonus_hunter_no_win_needed() {
        let mut ctx = base_context();
        ctx.won = false;
        ctx.max_simultaneous_bonuses = 3;
        let earned = check_achievements(&[], &ctx, 1);
        assert!(earned.iter().any(|e| e.id == AchievementId::BonusHunter));
    }

    #[test]
    fn test_explorer() {
        let mut ctx = base_context();
        ctx.maps_played = vec!["small_earth".to_string(), "mme".to_string()];
        ctx.total_maps_available = 2;
        let earned = check_achievements(&[], &ctx, 1);
        assert!(earned.iter().any(|e| e.id == AchievementId::Explorer));
    }

    #[test]
    fn test_build_newly_earned_views_empty() {
        let views = build_newly_earned_views(&[]);
        assert!(views.is_empty());
    }

    #[test]
    fn test_build_newly_earned_views_single() {
        let earned = vec![EarnedAchievement { id: AchievementId::FirstBlood, game_number: 1 }];
        let views = build_newly_earned_views(&earned);
        assert_eq!(views.len(), 1);
        assert!(views[0].earned);
        assert_eq!(views[0].earned_at, Some(1));
    }

    #[test]
    fn test_build_newly_earned_views_no_unearned_leak() {
        let earned = vec![
            EarnedAchievement { id: AchievementId::FirstBlood, game_number: 1 },
            EarnedAchievement { id: AchievementId::Speedrun, game_number: 1 },
        ];
        let views = build_newly_earned_views(&earned);
        assert_eq!(views.len(), 2);
        assert!(views.iter().all(|v| v.earned));
    }
}
