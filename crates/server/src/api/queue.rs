use axum::{Json, extract::State, http::StatusCode};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::AppState;
use crate::auth::AuthUser;
use crate::db;

#[derive(Deserialize)]
pub struct JoinQueueRequest {
    pub template: String,
}

#[derive(Serialize)]
pub struct QueueResponse {
    pub success: bool,
    pub message: String,
}

#[derive(Serialize)]
pub struct QueueStatusResponse {
    pub queued: bool,
    pub position: Option<usize>,
    pub estimated_wait_secs: Option<u32>,
    pub queue_size: usize,
    /// Set when matchmaking has found a match for this player.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched: Option<bool>,
    /// The game ID of the matched game.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub game_id: Option<Uuid>,
}

/// POST /api/queue/join -- Join the matchmaking queue.
pub async fn join_queue(
    auth: AuthUser,
    State(state): State<AppState>,
    Json(body): Json<JoinQueueRequest>,
) -> Result<Json<QueueResponse>, (StatusCode, String)> {
    // Look up the user's rating from DB if available, otherwise use default.
    let rating = if let Some(pool) = &state.db_pool {
        match db::get_user(pool, auth.user_id).await {
            Ok(Some(user)) => user.rating,
            _ => 1500.0,
        }
    } else {
        1500.0
    };

    let joined = state
        .matchmaking
        .enqueue(auth.user_id, auth.username.clone(), rating, body.template)
        .await;

    if joined {
        // Subscribe the user to their own user_id channel so they receive MatchFound events.
        // (The client should also subscribe via WebSocket to their user_id.)
        Ok(Json(QueueResponse {
            success: true,
            message: "Joined matchmaking queue".to_string(),
        }))
    } else {
        Ok(Json(QueueResponse {
            success: false,
            message: "Already in queue".to_string(),
        }))
    }
}

/// POST /api/queue/leave -- Leave the matchmaking queue.
pub async fn leave_queue(auth: AuthUser, State(state): State<AppState>) -> Json<QueueResponse> {
    let removed = state.matchmaking.dequeue(auth.user_id).await;

    Json(QueueResponse {
        success: removed,
        message: if removed {
            "Left matchmaking queue".to_string()
        } else {
            "Not in queue".to_string()
        },
    })
}

/// GET /api/queue/status -- Check queue position and estimated wait time.
pub async fn queue_status(
    auth: AuthUser,
    State(state): State<AppState>,
) -> Json<QueueStatusResponse> {
    let queue_size = state.matchmaking.queue_size().await;

    // Check if matchmaking already found a match for this player.
    if let Some(game_id) = state.matchmaking.take_match_result(auth.user_id).await {
        return Json(QueueStatusResponse {
            queued: false,
            position: None,
            estimated_wait_secs: None,
            queue_size,
            matched: Some(true),
            game_id: Some(game_id),
        });
    }

    if let Some((position, estimated_wait_secs)) =
        state.matchmaking.queue_status(auth.user_id).await
    {
        Json(QueueStatusResponse {
            queued: true,
            position: Some(position),
            estimated_wait_secs: Some(estimated_wait_secs),
            queue_size,
            matched: None,
            game_id: None,
        })
    } else {
        Json(QueueStatusResponse {
            queued: false,
            position: None,
            estimated_wait_secs: None,
            queue_size,
            matched: None,
            game_id: None,
        })
    }
}
