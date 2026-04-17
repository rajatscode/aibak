//! Map data model: territories, bonuses, adjacency, and game settings.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Pure geography: territories and bonuses, no gameplay config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Map {
    pub id: String,
    pub name: String,
    pub territories: Vec<Territory>,
    pub bonuses: Vec<Bonus>,
}

/// On-disk JSON format: geography + picking + settings.
/// Used only for loading map files; gameplay code uses `Board`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapFile {
    pub id: String,
    pub name: String,
    pub territories: Vec<Territory>,
    pub bonuses: Vec<Bonus>,
    pub picking: PickingConfig,
    pub settings: MapSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Territory {
    pub id: usize,
    pub name: String,
    pub bonus_id: usize,
    pub is_wasteland: bool,
    pub default_armies: u32,
    pub adjacent: Vec<usize>,
    #[serde(default)]
    pub visual: Option<TerritoryVisual>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerritoryVisual {
    pub path: String,
    pub label_pos: [f64; 2],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bonus {
    pub id: usize,
    pub name: String,
    pub value: u32,
    pub territory_ids: Vec<usize>,
    #[serde(default)]
    pub visual: Option<BonusVisual>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BonusVisual {
    pub color: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PickingConfig {
    pub num_picks: usize,
    pub method: PickingMethod,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PickingMethod {
    RandomWarlords,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapSettings {
    pub luck_pct: u32,
    pub base_income: u32,
    pub wasteland_armies: u32,
    pub unpicked_neutral_armies: u32,
    pub fog_of_war: bool,
    #[serde(default = "default_offense_kill_rate")]
    pub offense_kill_rate: f64,
    #[serde(default = "default_defense_kill_rate")]
    pub defense_kill_rate: f64,
}

fn default_offense_kill_rate() -> f64 {
    0.6
}

fn default_defense_kill_rate() -> f64 {
    0.7
}

impl MapFile {
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        let map_file: Self = serde_json::from_str(json)?;
        map_file.validate_adjacency();
        Ok(map_file)
    }

    pub fn load(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let json = std::fs::read_to_string(path)?;
        Ok(Self::from_json(&json)?)
    }

    fn validate_adjacency(&self) {
        for t in &self.territories {
            for &adj in &t.adjacent {
                assert!(
                    self.territories[adj].adjacent.contains(&t.id),
                    "One-directional adjacency: {} (id {}) lists {} (id {}) as adjacent, but not vice versa",
                    t.name, t.id, self.territories[adj].name, adj
                );
            }
        }
    }
}

impl Map {
    pub fn are_adjacent(&self, a: usize, b: usize) -> bool {
        self.territories[a].adjacent.contains(&b)
    }

    pub fn bonus_territories(&self, bonus_id: usize) -> &[usize] {
        &self.bonuses[bonus_id].territory_ids
    }

    pub fn territory_count(&self) -> usize {
        self.territories.len()
    }
}
