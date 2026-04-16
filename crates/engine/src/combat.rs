use crate::map::MapSettings;

/// Result of a single attack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CombatResult {
    /// Defenders killed by the attacker.
    pub defenders_killed: u32,
    /// Attackers killed by the defender.
    pub attackers_killed: u32,
    /// Whether the attacker captured the territory.
    pub captured: bool,
    /// Surviving attackers (only relevant if captured — these move into the territory).
    pub surviving_attackers: u32,
    /// Surviving defenders (0 if captured).
    pub surviving_defenders: u32,
}

/// Resolve an attack with deterministic (0% luck, straight round) combat.
///
/// Offense kill rate: each attacking army has a 60% chance to kill a defender.
/// Defense kill rate: each defending army has a 70% chance to kill an attacker.
/// At 0% luck these are deterministic: kills = round(armies * rate).
pub fn resolve_attack(
    attackers: u32,
    defenders: u32,
    settings: &MapSettings,
) -> CombatResult {
    assert!(attackers > 0, "must attack with at least 1 army");
    // Handle edge case where territory has been emptied by prior combat in the same turn.
    if defenders == 0 {
        return CombatResult {
            defenders_killed: 0,
            attackers_killed: 0,
            captured: true,
            surviving_attackers: attackers,
            surviving_defenders: 0,
        };
    }

    let offense_kills = straight_round(attackers as f64 * settings.offense_kill_rate);
    let defense_kills = straight_round(defenders as f64 * settings.defense_kill_rate);

    let actual_defenders_killed = offense_kills.min(defenders);
    let actual_attackers_killed = defense_kills.min(attackers);

    let remaining_defenders = defenders - actual_defenders_killed;
    let remaining_attackers = attackers - actual_attackers_killed;

    // Attacker captures if all defenders are killed and at least 1 attacker survives.
    let captured = remaining_defenders == 0 && remaining_attackers > 0;

    CombatResult {
        defenders_killed: actual_defenders_killed,
        attackers_killed: actual_attackers_killed,
        captured,
        surviving_attackers: remaining_attackers,
        surviving_defenders: remaining_defenders,
    }
}

/// Standard rounding: 0.5 rounds up.
fn straight_round(value: f64) -> u32 {
    (value + 0.5).floor() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_settings() -> MapSettings {
        MapSettings {
            luck_pct: 0,
            base_income: 5,
            wasteland_armies: 10,
            unpicked_neutral_armies: 4,
            fog_of_war: true,
            offense_kill_rate: 0.6,
            defense_kill_rate: 0.7,
        }
    }

    #[test]
    fn test_straight_round() {
        assert_eq!(straight_round(0.6), 1);
        assert_eq!(straight_round(1.2), 1);
        assert_eq!(straight_round(1.8), 2);
        assert_eq!(straight_round(2.4), 2);
        assert_eq!(straight_round(3.0), 3);
        assert_eq!(straight_round(0.5), 1);
        assert_eq!(straight_round(0.4), 0);
    }

    #[test]
    fn test_1v1_attack() {
        // 1 attacker: 0.6 -> 1 kill. 1 defender: 0.7 -> 1 kill.
        let r = resolve_attack(1, 1, &default_settings());
        assert_eq!(r.defenders_killed, 1);
        assert_eq!(r.attackers_killed, 1);
        assert!(!r.captured); // both die, no capture
    }

    #[test]
    fn test_2v1_capture() {
        // 2 atk: 1.2 -> 1 kill. 1 def: 0.7 -> 1 kill.
        let r = resolve_attack(2, 1, &default_settings());
        assert_eq!(r.defenders_killed, 1);
        assert_eq!(r.attackers_killed, 1);
        assert!(r.captured);
        assert_eq!(r.surviving_attackers, 1);
    }

    #[test]
    fn test_3v2_attack() {
        // 3 atk: 1.8 -> 2 kills. 2 def: 1.4 -> 1 kill.
        let r = resolve_attack(3, 2, &default_settings());
        assert_eq!(r.defenders_killed, 2);
        assert_eq!(r.attackers_killed, 1);
        assert!(r.captured);
        assert_eq!(r.surviving_attackers, 2);
    }

    #[test]
    fn test_4v2_attack() {
        // 4 atk: 2.4 -> 2 kills. 2 def: 1.4 -> 1 kill.
        let r = resolve_attack(4, 2, &default_settings());
        assert_eq!(r.defenders_killed, 2);
        assert_eq!(r.attackers_killed, 1);
        assert!(r.captured);
        assert_eq!(r.surviving_attackers, 3);
    }

    #[test]
    fn test_5v3_attack() {
        // 5 atk: 3.0 -> 3 kills. 3 def: 2.1 -> 2 kills.
        let r = resolve_attack(5, 3, &default_settings());
        assert_eq!(r.defenders_killed, 3);
        assert_eq!(r.attackers_killed, 2);
        assert!(r.captured);
        assert_eq!(r.surviving_attackers, 3);
    }

    #[test]
    fn test_large_attack_against_wasteland() {
        // 7 atk vs 10 def.
        // 7 * 0.6 = 4.2 -> 4 kills. 10 * 0.7 = 7.0 -> 7 kills.
        let r = resolve_attack(7, 10, &default_settings());
        assert_eq!(r.defenders_killed, 4);
        assert_eq!(r.attackers_killed, 7);
        assert!(!r.captured);
        assert_eq!(r.surviving_defenders, 6);
        assert_eq!(r.surviving_attackers, 0);
    }

    #[test]
    fn test_exact_wasteland_break() {
        // Need to kill 10 defenders: need ceil(10/0.6) attackers.
        // 17 * 0.6 = 10.2 -> 10 kills. 10 * 0.7 = 7 -> 7 kills.
        let r = resolve_attack(17, 10, &default_settings());
        assert_eq!(r.defenders_killed, 10);
        assert_eq!(r.attackers_killed, 7);
        assert!(r.captured);
        assert_eq!(r.surviving_attackers, 10);
    }
}
