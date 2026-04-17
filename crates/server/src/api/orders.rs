use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use strat_engine::orders::Order;

use crate::AppState;
use crate::auth::AuthUser;

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

/// JSON error response for API endpoints.
#[derive(Serialize)]
struct ErrorResponse {
    success: bool,
    message: String,
}

/// Wrapper for returning JSON errors from API handlers.
pub struct JsonError(pub StatusCode, pub String);

impl IntoResponse for JsonError {
    fn into_response(self) -> Response {
        let (status, msg) = (self.0, self.1);
        let error_response = ErrorResponse {
            success: false,
            message: msg,
        };
        (status, Json(error_response)).into_response()
    }
}

/// POST /api/games/:id/picks -- submit picks during picking phase.
pub async fn submit_picks(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(game_id): Path<Uuid>,
    Json(body): Json<SubmitPicksRequest>,
) -> Result<Json<ActionResponse>, JsonError> {
    tracing::info!("submit_picks: game={} user={} picks={:?}", game_id, auth.user_id, body.picks);
    let manager = state.require_game_manager()
        .map_err(|(status, msg)| JsonError(status, msg))?;
    match manager.submit_picks(game_id, auth.user_id, body.picks).await {
        Ok(()) => {
            tracing::info!("submit_picks: success for game={}", game_id);
            Ok(Json(ActionResponse {
                success: true,
                message: "picks submitted".to_string(),
            }))
        }
        Err(err) => {
            tracing::error!("submit_picks: error for game={}: {:?}", game_id, err);
            let (status, msg) = <_ as Into<(StatusCode, String)>>::into(err);
            Err(JsonError(status, msg))
        }
    }
}

/// POST /api/games/:id/orders -- submit orders during play phase.
pub async fn submit_orders(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(game_id): Path<Uuid>,
    Json(body): Json<SubmitOrdersRequest>,
) -> Result<Json<ActionResponse>, JsonError> {
    let manager = state.require_game_manager()
        .map_err(|(status, msg)| JsonError(status, msg))?;
    manager
        .submit_orders(game_id, auth.user_id, body.orders)
        .await
        .map_err(|err| {
            let (status, msg) = <_ as Into<(StatusCode, String)>>::into(err);
            JsonError(status, msg)
        })?;

    Ok(Json(ActionResponse {
        success: true,
        message: "orders submitted".to_string(),
    }))
}
