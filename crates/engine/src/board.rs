//! Board: a playable unit combining a Map with its configuration.

use serde::{Deserialize, Serialize};

use crate::map::{Map, MapFile, MapSettings, PickingConfig};

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

    pub fn settings(&self) -> &MapSettings {
        &self.config.settings
    }

    pub fn picking(&self) -> &PickingConfig {
        &self.config.picking
    }
}
