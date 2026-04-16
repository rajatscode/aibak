use std::path::PathBuf;

use axum::Json;
use axum::extract::Path;
use axum::http::StatusCode;
use serde::Serialize;

use strat_engine::map::Map;

/// Directory for user-created maps.
fn custom_maps_dir() -> PathBuf {
    PathBuf::from("maps/custom")
}

/// Directory for built-in maps.
fn builtin_maps_dir() -> PathBuf {
    PathBuf::from("maps")
}

#[derive(Serialize)]
pub struct MapInfo {
    pub id: String,
    pub name: String,
    pub territories: usize,
    pub bonuses: usize,
    pub is_custom: bool,
}

#[derive(Serialize)]
pub struct MapListResponse {
    pub maps: Vec<MapInfo>,
}

/// List all available maps (built-in + custom).
pub async fn list_maps() -> Json<MapListResponse> {
    let mut maps = Vec::new();

    // Built-in maps.
    if let Ok(entries) = std::fs::read_dir(builtin_maps_dir()) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json")
                && path.is_file()
                && let Ok(map) = Map::load(&path)
            {
                maps.push(MapInfo {
                    id: map.id.clone(),
                    name: map.name.clone(),
                    territories: map.territory_count(),
                    bonuses: map.bonuses.len(),
                    is_custom: false,
                });
            }
        }
    }

    // Custom maps.
    if let Ok(entries) = std::fs::read_dir(custom_maps_dir()) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json")
                && path.is_file()
                && let Ok(map) = Map::load(&path)
            {
                maps.push(MapInfo {
                    id: format!("custom/{}", map.id),
                    name: map.name.clone(),
                    territories: map.territory_count(),
                    bonuses: map.bonuses.len(),
                    is_custom: true,
                });
            }
        }
    }

    Json(MapListResponse { maps })
}

/// Save a custom map. Body is the raw map JSON.
pub async fn save_map(Json(map): Json<Map>) -> Result<Json<MapInfo>, (StatusCode, String)> {
    // Validate the map.
    if map.territories.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "Map has no territories".into()));
    }
    if map.bonuses.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "Map has no bonuses".into()));
    }
    if map.id.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "Map ID is required".into()));
    }

    // Validate connectivity.
    let n = map.territory_count();
    let mut visited = vec![false; n];
    let mut stack = vec![0usize];
    visited[0] = true;
    while let Some(tid) = stack.pop() {
        for &adj in &map.territories[tid].adjacent {
            if adj < n && !visited[adj] {
                visited[adj] = true;
                stack.push(adj);
            }
        }
    }
    let unreachable: Vec<usize> = visited
        .iter()
        .enumerate()
        .filter(|&(_, v)| !v)
        .map(|(i, _)| i)
        .collect();
    if !unreachable.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "Map is not fully connected. Unreachable territories: {:?}",
                unreachable
            ),
        ));
    }

    // Validate bidirectional adjacency.
    for t in &map.territories {
        for &adj in &t.adjacent {
            if adj >= n || !map.territories[adj].adjacent.contains(&t.id) {
                return Err((
                    StatusCode::BAD_REQUEST,
                    format!("Adjacency {} <-> {} is not bidirectional", t.id, adj),
                ));
            }
        }
    }

    // Save to disk.
    let dir = custom_maps_dir();
    std::fs::create_dir_all(&dir).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to create maps directory: {}", e),
        )
    })?;

    let filename = format!("{}.json", map.id.replace(['/', '\\', '.'], "_"));
    let path = dir.join(&filename);
    let json = serde_json::to_string_pretty(&map).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to serialize map: {}", e),
        )
    })?;
    std::fs::write(&path, json).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to write map file: {}", e),
        )
    })?;

    Ok(Json(MapInfo {
        id: format!("custom/{}", map.id),
        name: map.name.clone(),
        territories: map.territory_count(),
        bonuses: map.bonuses.len(),
        is_custom: true,
    }))
}

/// Delete a custom map.
pub async fn delete_map(
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let filename = format!("{}.json", id.replace(['/', '\\', '.'], "_"));
    let path = custom_maps_dir().join(&filename);

    if !path.exists() {
        return Err((StatusCode::NOT_FOUND, "Map not found".into()));
    }

    std::fs::remove_file(&path).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to delete map: {}", e),
        )
    })?;

    Ok(Json(serde_json::json!({"deleted": true})))
}
