use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use strat_engine::board::Board;
use strat_engine::fog;
use strat_engine::map::MapFile;
use strat_engine::state::GameState;

use crate::AppState;
use crate::auth::AuthUser;
use crate::db;

#[derive(Deserialize)]
pub struct CreateGameRequest {
    pub template: String,
}

#[derive(Serialize)]
pub struct GameResponse {
    pub id: Uuid,
    pub template: String,
    pub status: String,
    pub player_a: Option<Uuid>,
    pub player_b: Option<Uuid>,
    pub winner_id: Option<Uuid>,
    pub turn: i32,
    pub created_at: String,
    pub finished_at: Option<String>,
}

impl From<db::GameRow> for GameResponse {
    fn from(row: db::GameRow) -> Self {
        Self {
            id: row.id,
            template: row.template,
            status: row.status,
            player_a: row.player_a,
            player_b: row.player_b,
            winner_id: row.winner_id,
            turn: row.turn,
            created_at: row.created_at.to_rfc3339(),
            finished_at: row.finished_at.map(|t| t.to_rfc3339()),
        }
    }
}

#[derive(Serialize)]
pub struct GameStateResponse {
    pub game: GameResponse,
    pub state: Option<serde_json::Value>,
    pub pick_options: Option<Vec<usize>>,
    pub my_seat: Option<u8>,
}

#[derive(Deserialize)]
pub struct ListGamesQuery {
    pub status: Option<String>,
    pub limit: Option<i64>,
}

/// POST /api/games -- create a new game.
pub async fn create_game(
    auth: AuthUser,
    State(state): State<AppState>,
    Json(body): Json<CreateGameRequest>,
) -> Result<Json<GameResponse>, (StatusCode, String)> {
    let _pool = state.require_db()?;
    let manager = state.require_game_manager()?;

    let row = manager
        .create_game(auth.user_id, &body.template)
        .await
        .map_err(<_ as Into<(StatusCode, String)>>::into)?;

    Ok(Json(GameResponse::from(row)))
}

/// GET /api/games -- list games.
pub async fn list_games(
    auth: AuthUser,
    State(state): State<AppState>,
    Query(query): Query<ListGamesQuery>,
) -> Result<Json<Vec<GameResponse>>, (StatusCode, String)> {
    let pool = state.require_db()?;
    let limit = query.limit.unwrap_or(20);

    let rows = if let Some(status) = &query.status {
        db::list_games_by_status(pool, status, limit).await
    } else {
        db::list_user_games(pool, auth.user_id, limit).await
    };

    let rows = rows.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(rows.into_iter().map(GameResponse::from).collect()))
}

/// GET /api/games/:id -- get game state (fog-filtered for the requesting player).
pub async fn get_game(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(game_id): Path<Uuid>,
) -> Result<Json<GameStateResponse>, (StatusCode, String)> {
    let pool = state.require_db()?;

    let game = db::get_game(pool, game_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "game not found".to_string()))?;

    let my_seat = if game.player_a == Some(auth.user_id) {
        Some(0u8)
    } else if game.player_b == Some(auth.user_id) {
        Some(1u8)
    } else {
        None // Spectator.
    };

    // Fog-filter the game state for this player.
    let filtered_state = if let (Some(state_json), Some(map_json), Some(seat)) =
        (&game.state_json, &game.map_json, my_seat)
    {
        let game_state: GameState = serde_json::from_value(state_json.clone())
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        let map_file: MapFile = serde_json::from_value(map_json.clone())
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        let board = Board::from_map(map_file);
        let filtered = fog::fog_filter(&game_state, seat, &board);
        Some(
            serde_json::to_value(&filtered)
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?,
        )
    } else {
        game.state_json.clone()
    };

    let pick_options: Option<Vec<usize>> = game
        .pick_options
        .as_ref()
        .and_then(|v| serde_json::from_value(v.clone()).ok());

    Ok(Json(GameStateResponse {
        game: GameResponse::from(game),
        state: filtered_state,
        pick_options,
        my_seat,
    }))
}

/// POST /api/games/:id/join -- join an open game.
pub async fn join_game(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(game_id): Path<Uuid>,
) -> Result<Json<GameResponse>, (StatusCode, String)> {
    let manager = state.require_game_manager()?;
    let row = manager
        .join_game(game_id, auth.user_id)
        .await
        .map_err(<_ as Into<(StatusCode, String)>>::into)?;
    Ok(Json(GameResponse::from(row)))
}
