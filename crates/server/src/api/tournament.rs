use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::AppState;
use crate::auth::AuthUser;
use crate::db;
use crate::game::tournament::{Arena, ArenaParticipant, ArenaStatus};

// ── Request / response types ──

#[derive(Deserialize)]
pub struct CreateArenaRequest {
    pub name: String,
    pub template: String,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    #[serde(default = "default_time_control")]
    pub time_control_secs: u32,
}

fn default_time_control() -> u32 {
    300
}

#[derive(Serialize)]
pub struct ArenaResponse {
    pub id: Uuid,
    pub name: String,
    pub template: String,
    pub start_time: String,
    pub end_time: String,
    pub time_control_secs: u32,
    pub status: ArenaStatus,
    pub participant_count: usize,
    pub created_at: String,
}

#[derive(Serialize)]
pub struct ArenaDetailResponse {
    pub arena: ArenaResponse,
    pub participants: Vec<ParticipantView>,
}

#[derive(Serialize)]
pub struct ParticipantView {
    pub user_id: Uuid,
    pub username: String,
    pub avatar_url: Option<String>,
    pub score: i32,
    pub games_played: u32,
    pub wins: u32,
    pub current_streak: i32,
    pub rank: usize,
}

fn arena_to_response(arena: &Arena) -> ArenaResponse {
    ArenaResponse {
        id: arena.id,
        name: arena.name.clone(),
        template: arena.template.clone(),
        start_time: arena.start_time.to_rfc3339(),
        end_time: arena.end_time.to_rfc3339(),
        time_control_secs: arena.time_control_secs,
        status: arena.status(),
        participant_count: arena.participants.len(),
        created_at: arena.created_at.to_rfc3339(),
    }
}

fn participants_to_leaderboard(participants: &[ArenaParticipant]) -> Vec<ParticipantView> {
    let mut sorted: Vec<_> = participants.to_vec();
    // Sort by score descending, then wins descending as tiebreaker.
    sorted.sort_by(|a, b| b.score.cmp(&a.score).then(b.wins.cmp(&a.wins)));
    sorted
        .iter()
        .enumerate()
        .map(|(i, p)| ParticipantView {
            user_id: p.user_id,
            username: p.username.clone(),
            avatar_url: p.avatar_url.clone(),
            score: p.score,
            games_played: p.games_played,
            wins: p.wins,
            current_streak: p.current_streak,
            rank: i + 1,
        })
        .collect()
}

// ── Handlers ──

/// POST /api/arenas -- create an arena tournament (admin only).
pub async fn create_arena(
    auth: AuthUser,
    State(state): State<AppState>,
    Json(body): Json<CreateArenaRequest>,
) -> Result<Json<ArenaResponse>, (StatusCode, String)> {
    let pool = state.require_db()?;

    // Validate time window.
    if body.end_time <= body.start_time {
        return Err((
            StatusCode::BAD_REQUEST,
            "end_time must be after start_time".to_string(),
        ));
    }

    if body.name.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "arena name must not be empty".to_string(),
        ));
    }

    // For now, any authenticated user can create arenas.
    // TODO: Add admin role check when role system is implemented.
    let _creator = auth.user_id;

    let row = db::create_arena(
        pool,
        &body.name,
        &body.template,
        body.start_time,
        body.end_time,
        body.time_control_secs as i32,
    )
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let arena = Arena {
        id: row.id,
        name: row.name,
        template: row.template,
        start_time: row.start_time,
        end_time: row.end_time,
        time_control_secs: row.time_control_secs as u32,
        participants: Vec::new(),
        created_at: row.created_at,
    };

    Ok(Json(arena_to_response(&arena)))
}

/// GET /api/arenas -- list active and upcoming arenas.
pub async fn list_arenas(
    State(state): State<AppState>,
) -> Result<Json<Vec<ArenaResponse>>, (StatusCode, String)> {
    let pool = state.require_db()?;

    let rows = db::list_active_arenas(pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let mut arenas = Vec::new();
    for row in rows {
        let arena = Arena {
            id: row.id,
            name: row.name,
            template: row.template,
            start_time: row.start_time,
            end_time: row.end_time,
            time_control_secs: row.time_control_secs as u32,
            participants: Vec::new(), // participant count fetched separately
            created_at: row.created_at,
        };
        arenas.push(arena_to_response(&arena));
    }

    Ok(Json(arenas))
}

/// GET /api/arenas/:id -- arena details with leaderboard.
pub async fn get_arena(
    State(state): State<AppState>,
    Path(arena_id): Path<Uuid>,
) -> Result<Json<ArenaDetailResponse>, (StatusCode, String)> {
    let pool = state.require_db()?;

    let row = db::get_arena(pool, arena_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "arena not found".to_string()))?;

    let participant_rows = db::get_arena_participants(pool, arena_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let participants: Vec<ArenaParticipant> = participant_rows
        .into_iter()
        .map(|p| ArenaParticipant {
            user_id: p.user_id,
            username: p.username,
            avatar_url: p.avatar_url,
            score: p.score,
            games_played: p.games_played as u32,
            wins: p.wins as u32,
            current_streak: p.current_streak,
        })
        .collect();

    let arena = Arena {
        id: row.id,
        name: row.name,
        template: row.template,
        start_time: row.start_time,
        end_time: row.end_time,
        time_control_secs: row.time_control_secs as u32,
        participants: participants.clone(),
        created_at: row.created_at,
    };

    Ok(Json(ArenaDetailResponse {
        arena: arena_to_response(&arena),
        participants: participants_to_leaderboard(&participants),
    }))
}

/// POST /api/arenas/:id/join -- join an arena.
pub async fn join_arena(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(arena_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let pool = state.require_db()?;

    // Verify arena exists.
    let row = db::get_arena(pool, arena_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "arena not found".to_string()))?;

    // Check arena is active or upcoming.
    let arena = Arena {
        id: row.id,
        name: row.name,
        template: row.template,
        start_time: row.start_time,
        end_time: row.end_time,
        time_control_secs: row.time_control_secs as u32,
        participants: Vec::new(),
        created_at: row.created_at,
    };

    if arena.status() == ArenaStatus::Finished {
        return Err((
            StatusCode::BAD_REQUEST,
            "arena has already ended".to_string(),
        ));
    }

    db::join_arena(pool, arena_id, auth.user_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(serde_json::json!({
        "success": true,
        "message": format!("joined arena '{}'", arena.name),
    })))
}

/// GET /api/arenas/:id/leaderboard -- ranked participants.
pub async fn arena_leaderboard(
    State(state): State<AppState>,
    Path(arena_id): Path<Uuid>,
) -> Result<Json<Vec<ParticipantView>>, (StatusCode, String)> {
    let pool = state.require_db()?;

    // Verify arena exists.
    let _row = db::get_arena(pool, arena_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "arena not found".to_string()))?;

    let participant_rows = db::get_arena_participants(pool, arena_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let participants: Vec<ArenaParticipant> = participant_rows
        .into_iter()
        .map(|p| ArenaParticipant {
            user_id: p.user_id,
            username: p.username,
            avatar_url: p.avatar_url,
            score: p.score,
            games_played: p.games_played as u32,
            wins: p.wins as u32,
            current_streak: p.current_streak,
        })
        .collect();

    Ok(Json(participants_to_leaderboard(&participants)))
}
