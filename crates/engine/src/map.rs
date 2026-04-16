use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Map {
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

impl Map {
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    pub fn load(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let json = std::fs::read_to_string(path)?;
        Ok(Self::from_json(&json)?)
    }

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
