use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use strat_engine::analysis;
use strat_engine::map::Map;
use strat_engine::state::GameState;

use crate::auth::MaybeAuthUser;
use crate::db;
use crate::AppState;

// ── Response types ──

#[derive(Serialize)]
pub struct GameBrowserEntry {
    pub id: Uuid,
    pub template: String,
    pub status: String,
    pub player_a_name: Option<String>,
    pub player_b_name: Option<String>,
    pub turn: i32,
    pub created_at: String,
    pub finished_at: Option<String>,
    pub winner_name: Option<String>,
}

#[derive(Serialize)]
pub struct SpectateResponse {
    pub game: GameBrowserEntry,
    pub state: Option<serde_json::Value>,
    pub map: Option<serde_json::Value>,
    pub pick_options: Option<Vec<usize>>,
    pub win_probability: Option<WinProbView>,
}

#[derive(Serialize)]
pub struct WinProbView {
    pub player_0: f64,
    pub player_1: f64,
}

#[derive(Deserialize)]
pub struct BrowseQuery {
    pub limit: Option<i64>,
}

// ── Helper: resolve username from user_id ──

async fn resolve_username(pool: &sqlx::PgPool, user_id: Option<Uuid>) -> Option<String> {
    let uid = user_id?;
    db::get_user(pool, uid)
        .await
        .ok()
        .flatten()
        .map(|u| u.username)
}

async fn build_browser_entry(pool: &sqlx::PgPool, row: db::GameRow) -> GameBrowserEntry {
    let player_a_name = resolve_username(pool, row.player_a).await;
    let player_b_name = resolve_username(pool, row.player_b).await;
    let winner_name = resolve_username(pool, row.winner_id).await;
    GameBrowserEntry {
        id: row.id,
        template: row.template,
        status: row.status,
        player_a_name,
        player_b_name,
        turn: row.turn,
        created_at: row.created_at.to_rfc3339(),
        finished_at: row.finished_at.map(|t| t.to_rfc3339()),
        winner_name,
    }
}

// ── Route handlers ──

/// GET /api/games/active -- list active multiplayer games.
pub async fn active_games(
    State(state): State<AppState>,
    Query(query): Query<BrowseQuery>,
) -> Result<Json<Vec<GameBrowserEntry>>, (StatusCode, String)> {
    let pool = state.require_db()?;
    let limit = query.limit.unwrap_or(30);

    // Active = picking or active status.
    let mut entries = Vec::new();
    for status in &["active", "picking", "waiting"] {
        let rows = db::list_games_by_status(pool, status, limit)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        for row in rows {
            entries.push(build_browser_entry(pool, row).await);
        }
    }
    // Sort by most recently created.
    entries.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    entries.truncate(limit as usize);
    Ok(Json(entries))
}

/// GET /api/games/recent -- list recently completed games.
pub async fn recent_games(
    State(state): State<AppState>,
    Query(query): Query<BrowseQuery>,
) -> Result<Json<Vec<GameBrowserEntry>>, (StatusCode, String)> {
    let pool = state.require_db()?;
    let limit = query.limit.unwrap_or(20);
    let rows = db::list_games_by_status(pool, "finished", limit)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let mut entries = Vec::new();
    for row in rows {
        entries.push(build_browser_entry(pool, row).await);
    }
    Ok(Json(entries))
}

/// GET /api/games/:id/spectate -- full unfogged game state for spectators.
pub async fn spectate_game(
    _maybe_auth: MaybeAuthUser,
    State(state): State<AppState>,
    Path(game_id): Path<Uuid>,
) -> Result<Json<SpectateResponse>, (StatusCode, String)> {
    let pool = state.require_db()?;

    let game = db::get_game(pool, game_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "game not found".to_string()))?;

    // Compute win probability if we have state + map.
    let win_prob = if let (Some(state_json), Some(map_json)) = (&game.state_json, &game.map_json) {
        let game_state: Option<GameState> = serde_json::from_value(state_json.clone()).ok();
        let map: Option<Map> = serde_json::from_value(map_json.clone()).ok();
        if let (Some(gs), Some(m)) = (game_state, map) {
            let wp = analysis::quick_win_probability(&gs, &m);
            Some(WinProbView {
                player_0: wp.player_0,
                player_1: wp.player_1,
            })
        } else {
            None
        }
    } else {
        None
    };

    let pick_options: Option<Vec<usize>> = game
        .pick_options
        .as_ref()
        .and_then(|v| serde_json::from_value(v.clone()).ok());

    let entry = build_browser_entry(pool, game.clone()).await;

    Ok(Json(SpectateResponse {
        game: entry,
        state: game.state_json,
        map: game.map_json,
        pick_options,
        win_probability: win_prob,
    }))
}
