use axum::{extract::State, http::StatusCode, Json};

use crate::api::games::GameResponse;
use crate::db;
use crate::AppState;

/// GET /api/lobby -- open games waiting for an opponent.
pub async fn open_games(
    State(state): State<AppState>,
) -> Result<Json<Vec<GameResponse>>, (StatusCode, String)> {
    let pool = state.require_db()?;
    let rows = db::list_games_by_status(pool, "waiting", 50)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(rows.into_iter().map(GameResponse::from).collect()))
}
