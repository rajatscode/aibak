//! Opening book: named strategies for known maps.
//!
//! Each [`Opening`] describes a recommended picking priority and first-turn
//! plan so that newer players can learn common competitive approaches.

use serde::{Deserialize, Serialize};

/// A named opening strategy for a specific map.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Opening {
    pub name: String,
    pub description: String,
    /// Territory names in priority order (best first).
    pub recommended_picks: Vec<String>,
    pub first_turn_strategy: String,
}

/// Return the opening book for a given map id.
///
/// Currently only `"small_earth"` (Medium Earth) has hand-written openings.
/// Unknown maps return an empty list.
pub fn get_openings(map_id: &str) -> Vec<Opening> {
    match map_id {
        "small_earth" => small_earth_openings(),
        _ => Vec::new(),
    }
}

fn small_earth_openings() -> Vec<Opening> {
    vec![
        Opening {
            name: "South America Rush".into(),
            description:
                "Lock down South America early. With only 4 territories and a +2 bonus, \
                 it is the easiest continent to complete in the first 1-2 turns. Use the \
                 extra income to snowball into Africa or North America."
                    .into(),
            recommended_picks: vec![
                "Brazil".into(),
                "Argentina".into(),
                "Peru".into(),
                "Venezuela".into(),
            ],
            first_turn_strategy:
                "Deploy all armies onto one South America territory and sweep the \
                 remaining neutrals to complete the bonus on turn 1. If you already \
                 hold the whole continent, push into North Africa or Central America."
                    .into(),
        },
        Opening {
            name: "Australia Lock".into(),
            description:
                "Claim Oceania — the classic safe opening. Only one chokepoint \
                 (Indonesia-Siam) makes it trivially defensible. The +2 bonus is \
                 modest, but you lose almost nothing keeping it."
                    .into(),
            recommended_picks: vec![
                "Indonesia".into(),
                "Eastern Australia".into(),
                "Western Australia".into(),
                "New Guinea".into(),
            ],
            first_turn_strategy:
                "Capture any missing Oceania territory turn 1, then stack the \
                 chokepoint at Indonesia. Use spare income to probe into Southeast \
                 Asia when safe."
                    .into(),
        },
        Opening {
            name: "Africa Control".into(),
            description:
                "Africa offers a solid +3 bonus with 6 territories — a good balance \
                 of size and reward. Two main entry points (North Africa, East Africa) \
                 are manageable with focused defense."
                    .into(),
            recommended_picks: vec![
                "North Africa".into(),
                "Egypt".into(),
                "East Africa".into(),
                "Congo".into(),
                "South Africa".into(),
                "Madagascar".into(),
            ],
            first_turn_strategy:
                "Concentrate armies to sweep neutrals in Africa. Prioritize the \
                 borders with Europe (North Africa) and the Middle East (Egypt/East \
                 Africa) once the bonus is complete."
                    .into(),
        },
        Opening {
            name: "Europe Gambit".into(),
            description:
                "Europe is risky — 7 territories with multiple entry points — but \
                 its +5 bonus is the highest reward-per-territory on the map. If you \
                 can hold it, the income advantage is crushing."
                    .into(),
            recommended_picks: vec![
                "Ukraine".into(),
                "Northern Europe".into(),
                "Southern Europe".into(),
                "Scandinavia".into(),
                "Western Europe".into(),
                "Great Britain".into(),
                "Iceland".into(),
            ],
            first_turn_strategy:
                "Deploy everything into the interior (Northern Europe or Ukraine) \
                 and clear neutrals aggressively. Expect early pressure from Asia \
                 and Africa — keep a reserve for Ukraine."
                    .into(),
        },
        Opening {
            name: "Asia Long Game".into(),
            description:
                "Spread across Asia for a massive +7 late-game bonus. This is a \
                 patient strategy: you will not complete the continent quickly, but \
                 controlling key territories denies opponents expansion while you \
                 build an army advantage."
                    .into(),
            recommended_picks: vec![
                "Siam".into(),
                "India".into(),
                "China".into(),
                "Middle East".into(),
                "Ural".into(),
                "Siberia".into(),
            ],
            first_turn_strategy:
                "Do not rush the full bonus. Instead, capture nearby neutrals to \
                 grow your territory count and card income. Focus on chokepoints \
                 (Siam, Middle East, Ural) to slow opponents while you consolidate."
                    .into(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_earth_has_openings() {
        let openings = get_openings("small_earth");
        assert_eq!(openings.len(), 5);
        assert_eq!(openings[0].name, "South America Rush");
    }

    #[test]
    fn unknown_map_returns_empty() {
        assert!(get_openings("unknown_map").is_empty());
    }
}
