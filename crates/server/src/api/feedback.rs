use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::AppState;
use crate::auth::{AuthUser, MaybeAuthUser};

#[derive(Serialize, sqlx::FromRow)]
pub struct FeedbackRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub content: String,
    pub upvotes: i32,
    pub downvotes: i32,
    pub resolved: bool,
    pub resolved_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub username: String,
}

#[derive(Deserialize)]
pub struct SubmitFeedback {
    pub content: String,
}

#[derive(Deserialize)]
pub struct FeedbackListQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Deserialize)]
pub struct VoteRequest {
    pub direction: i32,
}

#[derive(Serialize)]
pub struct FeedbackResponse {
    id: Uuid,
    success: bool,
}

#[derive(Serialize)]
pub struct VoteResponse {
    action: String,
}

#[derive(Serialize)]
pub struct DeleteResponse {
    success: bool,
}

#[derive(Serialize)]
pub struct ResolveResponse {
    resolved: bool,
}

async fn update_vote_counts(
    pool: &sqlx::PgPool,
    feedback_id: Uuid,
    up_delta: i32,
    down_delta: i32,
) -> Result<(), (StatusCode, String)> {
    sqlx::query("UPDATE feedback SET upvotes = upvotes + $1, downvotes = downvotes + $2 WHERE id = $3")
        .bind(up_delta)
        .bind(down_delta)
        .bind(feedback_id)
        .execute(pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(())
}

/// POST /api/feedback -- submit feedback (auth required).
pub async fn submit_feedback(
    auth: AuthUser,
    State(state): State<AppState>,
    Json(body): Json<SubmitFeedback>,
) -> Result<Json<FeedbackResponse>, (StatusCode, String)> {
    let pool = state.require_db()?;

    let content = body.content.trim();
    if content.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "content cannot be empty".to_string()));
    }
    if content.len() > 2000 {
        return Err((
            StatusCode::BAD_REQUEST,
            "content must be 2000 characters or fewer".to_string(),
        ));
    }

    let id = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO feedback (user_id, content) VALUES ($1, $2) RETURNING id",
    )
    .bind(auth.user_id)
    .bind(content)
    .fetch_one(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(FeedbackResponse { id, success: true }))
}

/// GET /api/feedback -- list feedback sorted by score desc, then newest.
pub async fn list_feedback(
    _auth: MaybeAuthUser,
    State(state): State<AppState>,
    Query(query): Query<FeedbackListQuery>,
) -> Result<Json<Vec<FeedbackRow>>, (StatusCode, String)> {
    let pool = state.require_db()?;
    let limit = query.limit.unwrap_or(20).min(100);
    let offset = query.offset.unwrap_or(0);

    let rows = sqlx::query_as::<_, FeedbackRow>(
        "SELECT f.id, f.user_id, f.content, f.upvotes, f.downvotes, f.resolved, f.resolved_at, f.created_at, u.username \
         FROM feedback f JOIN users u ON f.user_id = u.id \
         ORDER BY f.resolved ASC, (f.upvotes - f.downvotes) DESC, f.created_at DESC \
         LIMIT $1 OFFSET $2",
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(rows))
}

/// POST /api/feedback/:id/vote -- vote on feedback (auth required).
/// Toggle if same direction, otherwise update.
pub async fn vote_feedback(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(feedback_id): Path<Uuid>,
    Json(body): Json<VoteRequest>,
) -> Result<Json<VoteResponse>, (StatusCode, String)> {
    let pool = state.require_db()?;

    if body.direction != 1 && body.direction != -1 {
        return Err((
            StatusCode::BAD_REQUEST,
            "direction must be 1 or -1".to_string(),
        ));
    }

    // Check feedback exists.
    let exists = sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM feedback WHERE id = $1)")
        .bind(feedback_id)
        .fetch_one(pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if !exists {
        return Err((StatusCode::NOT_FOUND, "feedback not found".to_string()));
    }

    // Check for existing vote.
    let existing = sqlx::query_as::<_, (Uuid, i32)>(
        "SELECT id, direction FROM feedback_votes WHERE user_id = $1 AND feedback_id = $2",
    )
    .bind(auth.user_id)
    .bind(feedback_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let action = match existing {
        Some((vote_id, old_direction)) if old_direction == body.direction => {
            // Same direction: toggle off (remove vote).
            sqlx::query("DELETE FROM feedback_votes WHERE id = $1")
                .bind(vote_id)
                .execute(pool)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

            let (up, down) = if body.direction == 1 { (-1, 0) } else { (0, -1) };
            update_vote_counts(pool, feedback_id, up, down).await?;
            "removed"
        }
        Some((vote_id, _old_direction)) => {
            // Different direction: flip the vote.
            sqlx::query("UPDATE feedback_votes SET direction = $1 WHERE id = $2")
                .bind(body.direction)
                .bind(vote_id)
                .execute(pool)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

            let (up, down) = if body.direction == 1 { (1, -1) } else { (-1, 1) };
            update_vote_counts(pool, feedback_id, up, down).await?;
            "changed"
        }
        None => {
            // New vote.
            sqlx::query(
                "INSERT INTO feedback_votes (user_id, feedback_id, direction) VALUES ($1, $2, $3)",
            )
            .bind(auth.user_id)
            .bind(feedback_id)
            .bind(body.direction)
            .execute(pool)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

            let (up, down) = if body.direction == 1 { (1, 0) } else { (0, 1) };
            update_vote_counts(pool, feedback_id, up, down).await?;
            "voted"
        }
    };

    Ok(Json(VoteResponse { action: action.to_string() }))
}

/// DELETE /api/feedback/:id -- delete own feedback only (auth required).
pub async fn delete_feedback(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(feedback_id): Path<Uuid>,
) -> Result<Json<DeleteResponse>, (StatusCode, String)> {
    let pool = state.require_db()?;

    // Check ownership.
    let owner_id = sqlx::query_scalar::<_, Uuid>("SELECT user_id FROM feedback WHERE id = $1")
        .bind(feedback_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "feedback not found".to_string()))?;

    if owner_id != auth.user_id {
        return Err((
            StatusCode::FORBIDDEN,
            "you can only delete your own feedback".to_string(),
        ));
    }

    // Delete votes first (foreign key), then feedback.
    sqlx::query("DELETE FROM feedback_votes WHERE feedback_id = $1")
        .bind(feedback_id)
        .execute(pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    sqlx::query("DELETE FROM feedback WHERE id = $1")
        .bind(feedback_id)
        .execute(pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(DeleteResponse { success: true }))
}

/// POST /api/feedback/:id/resolve -- toggle resolved status (author only).
pub async fn resolve_feedback(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(feedback_id): Path<Uuid>,
) -> Result<Json<ResolveResponse>, (StatusCode, String)> {
    let pool = state.require_db()?;

    // Check ownership.
    let owner_id = sqlx::query_scalar::<_, Uuid>("SELECT user_id FROM feedback WHERE id = $1")
        .bind(feedback_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "feedback not found".to_string()))?;

    if owner_id != auth.user_id {
        return Err((
            StatusCode::FORBIDDEN,
            "you can only resolve your own feedback".to_string(),
        ));
    }

    // Toggle resolved.
    let new_resolved = sqlx::query_scalar::<_, bool>(
        "UPDATE feedback SET resolved = NOT resolved, resolved_at = CASE WHEN resolved THEN NULL ELSE now() END WHERE id = $1 RETURNING resolved",
    )
    .bind(feedback_id)
    .fetch_one(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(ResolveResponse { resolved: new_resolved }))
}
