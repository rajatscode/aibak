//! Board: a playable unit combining a Map with its configuration.

use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::map::{Map, MapFile, MapSettings, PickingConfig};

/// On-disk format for a board JSON file.
#[derive(Debug, Clone, Deserialize)]
struct BoardFile {
    id: String,
    name: String,
    map_id: String,
    config: BoardConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardConfig {
    pub picking: PickingConfig,
    pub settings: MapSettings,
}

/// A Board = Map + BoardConfig. The playable unit.
/// Single source of truth for gameplay settings — Map is pure geography.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Board {
    pub id: String,
    pub name: String,
    pub map: Map,
    pub config: BoardConfig,
}

impl Board {
    /// Create a Board from a MapFile (the on-disk JSON format).
    /// Extracts settings and picking into BoardConfig; Map becomes pure geography.
    pub fn from_map(mf: MapFile) -> Self {
        let config = BoardConfig {
            picking: mf.picking,
            settings: mf.settings,
        };
        let map = Map {
            id: mf.id.clone(),
            name: mf.name.clone(),
            territories: mf.territories,
            bonuses: mf.bonuses,
        };
        Board {
            id: mf.id,
            name: mf.name,
            map,
            config,
        }
    }

    /// Load a Board from a board JSON file + map directory.
    ///
    /// The board file contains the board id/name, a `map_id` referencing a map,
    /// and a `config` with picking + settings. The map is loaded from
    /// `maps_dir/<map_id>.json`.
    pub fn load(board_path: &Path, maps_dir: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let board_json = std::fs::read_to_string(board_path)?;
        let bf: BoardFile = serde_json::from_str(&board_json)?;

        let map_path = maps_dir.join(format!("{}.json", bf.map_id));
        let map_json = std::fs::read_to_string(&map_path)?;
        let map: Map = serde_json::from_str(&map_json)?;

        Ok(Board {
            id: bf.id,
            name: bf.name,
            map,
            config: bf.config,
        })
    }

    pub fn settings(&self) -> &MapSettings {
        &self.config.settings
    }

    pub fn picking(&self) -> &PickingConfig {
        &self.config.picking
    }
}
