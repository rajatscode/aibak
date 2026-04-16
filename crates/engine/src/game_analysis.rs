//! Post-game analysis for completed games.
//!
//! Analyzes a completed game's state history and produces insights including
//! territory/income charts, key moments, efficiency metrics, and bonus timelines.

use serde::{Deserialize, Serialize};

use crate::map::Map;
use crate::state::{GameState, Phase, PlayerId};
use crate::turn::TurnEvent;

// ── Public types ──

/// Full post-game analysis of a completed game.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameAnalysis {
    /// Total turns played.
    pub turns_played: u32,
    /// Territory counts per turn: (player_territories, enemy_territories).
    pub territory_control_over_time: Vec<(u32, u32)>,
    /// Income per turn: (player_income, enemy_income).
    pub income_over_time: Vec<(u32, u32)>,
    /// Significant moments during the game.
    pub key_moments: Vec<KeyMoment>,
    /// Armies deployed per territory captured (lower is more efficient).
    pub player_efficiency: f64,
    /// The single biggest attack of the game.
    pub biggest_attack: Option<AttackSummary>,
    /// Per-turn, per-bonus: did the player own it?
    pub bonus_control_timeline: Vec<Vec<bool>>,
}

/// A significant moment during the game.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyMoment {
    /// Turn number when this moment occurred.
    pub turn: u32,
    /// Human-readable description.
    pub description: String,
    /// How much the win probability shifted (positive = towards player 0).
    pub win_prob_change: f64,
    /// Classification of the moment.
    pub moment_type: MomentType,
}

/// Classification of key moments.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MomentType {
    /// Win probability crossed 50%.
    TurningPoint,
    /// >15% win probability change in one turn.
    BigSwing,
    /// Player completed a bonus.
    BonusCompleted,
    /// Player lost a previously-held bonus.
    BonusLost,
    /// Player could have captured a bonus but didn't.
    MissedOpportunity,
}

/// Summary of a single attack.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttackSummary {
    /// Turn the attack occurred.
    pub turn: u32,
    /// Attacking player.
    pub player: PlayerId,
    /// Source territory name.
    pub from_name: String,
    /// Target territory name.
    pub to_name: String,
    /// Armies committed to the attack.
    pub armies: u32,
    /// Defending armies.
    pub defenders: u32,
    /// Whether the territory was captured.
    pub captured: bool,
}

// ── Analysis function ──

/// Analyze a completed game and produce a full post-game report.
///
/// # Arguments
/// - `state_history` - Snapshots of the game state before each turn's orders.
/// - `win_prob_history` - Player 0 win probability recorded after each turn.
/// - `turn_events` - Events generated during each turn's resolution.
/// - `map` - The map the game was played on.
pub fn analyze_game(
    state_history: &[GameState],
    win_prob_history: &[f64],
    turn_events: &[Vec<TurnEvent>],
    map: &Map,
) -> GameAnalysis {
    let turns_played = state_history.len() as u32;

    // Territory control over time.
    let territory_control_over_time: Vec<(u32, u32)> = state_history
        .iter()
        .map(|s| {
            (
                s.territory_count_for(0) as u32,
                s.territory_count_for(1) as u32,
            )
        })
        .collect();

    // Income over time.
    let income_over_time: Vec<(u32, u32)> = state_history
        .iter()
        .map(|s| (s.income(0, map), s.income(1, map)))
        .collect();

    // Bonus control timeline.
    let bonus_control_timeline: Vec<Vec<bool>> = state_history
        .iter()
        .map(|s| {
            map.bonuses
                .iter()
                .map(|b| {
                    b.territory_ids
                        .iter()
                        .all(|&tid| s.territory_owners[tid] == 0)
                })
                .collect()
        })
        .collect();

    // Key moments.
    let key_moments = find_key_moments(
        state_history,
        win_prob_history,
        &bonus_control_timeline,
        turn_events,
        map,
    );

    // Player efficiency: total armies deployed by player 0 / territories captured.
    let (total_deployed, total_captured) = compute_player_efficiency(turn_events);
    let player_efficiency = if total_captured > 0 {
        total_deployed as f64 / total_captured as f64
    } else {
        0.0
    };

    // Biggest attack across all turns.
    let biggest_attack = find_biggest_attack(turn_events, map);

    GameAnalysis {
        turns_played,
        territory_control_over_time,
        income_over_time,
        key_moments,
        player_efficiency,
        biggest_attack,
        bonus_control_timeline,
    }
}

/// Find key moments in the game based on win probability shifts and bonus changes.
fn find_key_moments(
    state_history: &[GameState],
    win_prob_history: &[f64],
    bonus_timeline: &[Vec<bool>],
    _turn_events: &[Vec<TurnEvent>],
    map: &Map,
) -> Vec<KeyMoment> {
    let mut moments = Vec::new();

    // Win probability-based moments.
    for i in 1..win_prob_history.len() {
        let prev = win_prob_history[i - 1];
        let curr = win_prob_history[i];
        let change = curr - prev;
        let turn = i as u32;

        // Turning point: win prob crossed 50%.
        if (prev < 0.5 && curr >= 0.5) || (prev >= 0.5 && curr < 0.5) {
            let direction = if curr >= 0.5 {
                "Player took the lead"
            } else {
                "AI seized the advantage"
            };
            moments.push(KeyMoment {
                turn,
                description: format!("{} (win probability crossed 50%)", direction),
                win_prob_change: change,
                moment_type: MomentType::TurningPoint,
            });
        }
        // Big swing: >15% change in one turn.
        else if change.abs() > 0.15 {
            let direction = if change > 0.0 {
                "Major swing in player's favor"
            } else {
                "Major swing against player"
            };
            moments.push(KeyMoment {
                turn,
                description: format!(
                    "{} ({:+.0}% win probability)",
                    direction,
                    change * 100.0
                ),
                win_prob_change: change,
                moment_type: MomentType::BigSwing,
            });
        }
    }

    // Bonus completion/loss moments.
    for turn_idx in 1..bonus_timeline.len() {
        let prev = &bonus_timeline[turn_idx - 1];
        let curr = &bonus_timeline[turn_idx];
        let turn = turn_idx as u32;

        for (bonus_idx, (was_owned, is_owned)) in prev.iter().zip(curr.iter()).enumerate() {
            if !was_owned && *is_owned {
                let bonus_name = &map.bonuses[bonus_idx].name;
                let bonus_value = map.bonuses[bonus_idx].value;
                let wp_change = if turn_idx < win_prob_history.len() && turn_idx > 0 {
                    win_prob_history[turn_idx] - win_prob_history[turn_idx - 1]
                } else {
                    0.0
                };
                moments.push(KeyMoment {
                    turn,
                    description: format!(
                        "Completed {} bonus (+{} income)",
                        bonus_name, bonus_value
                    ),
                    win_prob_change: wp_change,
                    moment_type: MomentType::BonusCompleted,
                });
            } else if *was_owned && !is_owned {
                let bonus_name = &map.bonuses[bonus_idx].name;
                let bonus_value = map.bonuses[bonus_idx].value;
                let wp_change = if turn_idx < win_prob_history.len() && turn_idx > 0 {
                    win_prob_history[turn_idx] - win_prob_history[turn_idx - 1]
                } else {
                    0.0
                };
                moments.push(KeyMoment {
                    turn,
                    description: format!(
                        "Lost {} bonus (-{} income)",
                        bonus_name, bonus_value
                    ),
                    win_prob_change: wp_change,
                    moment_type: MomentType::BonusLost,
                });
            }
        }
    }

    // Missed opportunity detection: check if AI had only 1 territory left in a
    // bonus and the player didn't attack it despite having adjacent forces.
    for turn_idx in 0..state_history.len().saturating_sub(1) {
        let state = &state_history[turn_idx];
        let turn = turn_idx as u32 + 1;

        // Skip if not in play phase.
        if state.phase != Phase::Play {
            continue;
        }

        for bonus in &map.bonuses {
            // Count how many territories in this bonus the player owns.
            let player_owned: Vec<usize> = bonus
                .territory_ids
                .iter()
                .filter(|&&tid| state.territory_owners[tid] == 0)
                .copied()
                .collect();
            let enemy_in_bonus: Vec<usize> = bonus
                .territory_ids
                .iter()
                .filter(|&&tid| state.territory_owners[tid] == 1)
                .copied()
                .collect();

            // If player owns all but 1 territory in this bonus...
            if enemy_in_bonus.len() == 1
                && player_owned.len() + 1 == bonus.territory_ids.len()
            {
                let target = enemy_in_bonus[0];
                // Check if player had an adjacent territory with enough armies.
                let can_attack = map.territories[target]
                    .adjacent
                    .iter()
                    .any(|&adj| {
                        state.territory_owners[adj] == 0
                            && state.territory_armies[adj] > state.territory_armies[target]
                    });

                if can_attack {
                    // Check if the next state still doesn't have the bonus.
                    if let Some(next_state) = state_history.get(turn_idx + 1) {
                        let still_missing = next_state.territory_owners[target] != 0;
                        if still_missing {
                            let wp_change =
                                if turn_idx < win_prob_history.len() && turn_idx > 0 {
                                    win_prob_history[turn_idx]
                                        - win_prob_history[turn_idx - 1]
                                } else {
                                    0.0
                                };
                            moments.push(KeyMoment {
                                turn,
                                description: format!(
                                    "Missed chance to complete {} bonus",
                                    bonus.name
                                ),
                                win_prob_change: wp_change,
                                moment_type: MomentType::MissedOpportunity,
                            });
                        }
                    }
                }
            }
        }
    }

    // Sort by turn number, then by absolute win_prob_change descending.
    moments.sort_by(|a, b| {
        a.turn
            .cmp(&b.turn)
            .then_with(|| {
                b.win_prob_change
                    .abs()
                    .partial_cmp(&a.win_prob_change.abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });

    moments
}

/// Compute total armies deployed and territories captured by player 0.
fn compute_player_efficiency(turn_events: &[Vec<TurnEvent>]) -> (u32, u32) {
    let mut total_deployed = 0u32;
    let mut total_captured = 0u32;

    for events in turn_events {
        for event in events {
            match event {
                TurnEvent::Deploy {
                    player: 0, armies, ..
                } => {
                    total_deployed += armies;
                }
                TurnEvent::Capture { player: 0, .. } => {
                    total_captured += 1;
                }
                _ => {}
            }
        }
    }

    (total_deployed, total_captured)
}

/// Find the biggest attack across all turns (by army count).
fn find_biggest_attack(turn_events: &[Vec<TurnEvent>], map: &Map) -> Option<AttackSummary> {
    let mut biggest: Option<AttackSummary> = None;

    for (turn_idx, events) in turn_events.iter().enumerate() {
        for event in events {
            if let TurnEvent::Attack {
                player,
                from,
                to,
                armies,
                defenders,
                captured,
                ..
            } = event
            {
                let dominated = biggest.as_ref().map_or(true, |b| *armies > b.armies);
                if dominated {
                    biggest = Some(AttackSummary {
                        turn: turn_idx as u32 + 1,
                        player: *player,
                        from_name: map
                            .territories
                            .get(*from)
                            .map(|t| t.name.clone())
                            .unwrap_or_else(|| format!("#{}", from)),
                        to_name: map
                            .territories
                            .get(*to)
                            .map(|t| t.name.clone())
                            .unwrap_or_else(|| format!("#{}", to)),
                        armies: *armies,
                        defenders: *defenders,
                        captured: *captured,
                    });
                }
            }
        }
    }

    biggest
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Bonus, MapSettings, PickingConfig, PickingMethod, Territory};
    use crate::state::NEUTRAL;

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
                    adjacent: vec![0, 2],
                    visual: None,
                },
                Territory {
                    id: 2,
                    name: "C".into(),
                    bonus_id: 1,
                    is_wasteland: false,
                    default_armies: 2,
                    adjacent: vec![1, 3],
                    visual: None,
                },
                Territory {
                    id: 3,
                    name: "D".into(),
                    bonus_id: 1,
                    is_wasteland: false,
                    default_armies: 2,
                    adjacent: vec![2],
                    visual: None,
                },
            ],
            bonuses: vec![
                Bonus {
                    id: 0,
                    name: "Left".into(),
                    value: 2,
                    territory_ids: vec![0, 1],
                    visual: None,
                },
                Bonus {
                    id: 1,
                    name: "Right".into(),
                    value: 2,
                    territory_ids: vec![2, 3],
                    visual: None,
                },
            ],
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
    fn test_analyze_empty_game() {
        let map = test_map();
        let analysis = analyze_game(&[], &[], &[], &map);
        assert_eq!(analysis.turns_played, 0);
        assert!(analysis.territory_control_over_time.is_empty());
        assert!(analysis.income_over_time.is_empty());
        assert!(analysis.key_moments.is_empty());
        assert!(analysis.biggest_attack.is_none());
    }

    #[test]
    fn test_analyze_territory_control() {
        let map = test_map();
        let mut s1 = GameState::new(&map);
        s1.territory_owners = vec![0, 0, 1, 1];
        s1.phase = Phase::Play;

        let mut s2 = s1.clone();
        s2.territory_owners = vec![0, 0, 0, 1]; // player captured C

        let analysis = analyze_game(&[s1, s2], &[0.5, 0.65], &[vec![]], &map);
        assert_eq!(analysis.turns_played, 2);
        assert_eq!(analysis.territory_control_over_time[0], (2, 2));
        assert_eq!(analysis.territory_control_over_time[1], (3, 1));
    }

    #[test]
    fn test_analyze_detects_bonus_completion() {
        let map = test_map();
        let mut s1 = GameState::new(&map);
        s1.territory_owners = vec![0, NEUTRAL, 1, 1];
        s1.phase = Phase::Play;

        let mut s2 = s1.clone();
        s2.territory_owners = vec![0, 0, 1, 1]; // player now owns both Left bonus territories

        let analysis = analyze_game(&[s1, s2], &[0.4, 0.55], &[vec![]], &map);
        let bonus_moments: Vec<_> = analysis
            .key_moments
            .iter()
            .filter(|m| matches!(m.moment_type, MomentType::BonusCompleted))
            .collect();
        assert_eq!(bonus_moments.len(), 1);
        assert!(bonus_moments[0].description.contains("Left"));
    }

    #[test]
    fn test_analyze_biggest_attack() {
        let map = test_map();
        let events = vec![vec![
            TurnEvent::Attack {
                player: 0,
                from: 1,
                to: 2,
                armies: 8,
                defenders: 3,
                attackers_killed: 1,
                defenders_killed: 3,
                captured: true,
                surviving_attackers: 7,
            },
            TurnEvent::Attack {
                player: 1,
                from: 3,
                to: 2,
                armies: 4,
                defenders: 7,
                attackers_killed: 4,
                defenders_killed: 2,
                captured: false,
                surviving_attackers: 0,
            },
        ]];

        let s = GameState::new(&map);
        let analysis = analyze_game(&[s], &[0.5], &events, &map);
        let biggest = analysis.biggest_attack.unwrap();
        assert_eq!(biggest.armies, 8);
        assert_eq!(biggest.from_name, "B");
        assert_eq!(biggest.to_name, "C");
        assert!(biggest.captured);
    }

    #[test]
    fn test_player_efficiency() {
        let map = test_map();
        let events = vec![vec![
            TurnEvent::Deploy {
                player: 0,
                territory: 0,
                armies: 5,
            },
            TurnEvent::Deploy {
                player: 0,
                territory: 1,
                armies: 3,
            },
            TurnEvent::Capture {
                player: 0,
                territory: 2,
            },
            TurnEvent::Capture {
                player: 0,
                territory: 3,
            },
        ]];

        let s = GameState::new(&map);
        let analysis = analyze_game(&[s], &[0.5], &events, &map);
        // 8 armies deployed, 2 territories captured => 4.0 efficiency
        assert!((analysis.player_efficiency - 4.0).abs() < 0.01);
    }

    #[test]
    fn test_turning_point_detection() {
        let map = test_map();
        let s = GameState::new(&map);
        // Win prob goes from 0.45 to 0.55 (crosses 50%).
        let analysis = analyze_game(&[s], &[0.45, 0.55], &[vec![]], &map);
        let turning = analysis
            .key_moments
            .iter()
            .any(|m| matches!(m.moment_type, MomentType::TurningPoint));
        assert!(turning);
    }

    #[test]
    fn test_big_swing_detection() {
        let map = test_map();
        let s = GameState::new(&map);
        // Win prob drops from 0.8 to 0.6 (20% swing, doesn't cross 50%).
        let analysis = analyze_game(&[s], &[0.8, 0.6], &[vec![]], &map);
        let swing = analysis
            .key_moments
            .iter()
            .any(|m| matches!(m.moment_type, MomentType::BigSwing));
        assert!(swing);
    }
}
