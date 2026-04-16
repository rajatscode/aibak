use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use strat_engine::orders::Order;

use crate::auth::AuthUser;
use crate::AppState;

#[derive(Deserialize)]
pub struct SubmitPicksRequest {
    pub picks: Vec<usize>,
}

#[derive(Deserialize)]
pub struct SubmitOrdersRequest {
    pub orders: Vec<Order>,
}

#[derive(Serialize)]
pub struct ActionResponse {
    pub success: bool,
    pub message: String,
}

/// POST /api/games/:id/picks -- submit picks during picking phase.
pub async fn submit_picks(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(game_id): Path<Uuid>,
    Json(body): Json<SubmitPicksRequest>,
) -> Result<Json<ActionResponse>, (StatusCode, String)> {
    let manager = state.require_game_manager()?;
    manager
        .submit_picks(game_id, auth.user_id, body.picks)
        .await
        .map_err(|e| <_ as Into<(StatusCode, String)>>::into(e))?;

    Ok(Json(ActionResponse {
        success: true,
        message: "picks submitted".to_string(),
    }))
}

/// POST /api/games/:id/orders -- submit orders during play phase.
pub async fn submit_orders(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(game_id): Path<Uuid>,
    Json(body): Json<SubmitOrdersRequest>,
) -> Result<Json<ActionResponse>, (StatusCode, String)> {
    let manager = state.require_game_manager()?;
    manager
        .submit_orders(game_id, auth.user_id, body.orders)
        .await
        .map_err(|e| <_ as Into<(StatusCode, String)>>::into(e))?;

    Ok(Json(ActionResponse {
        success: true,
        message: "orders submitted".to_string(),
    }))
}
