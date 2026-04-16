use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::AppState;
use crate::db;

#[derive(Serialize)]
pub struct PlayerProfile {
    pub id: Uuid,
    pub username: String,
    pub avatar_url: Option<String>,
    pub rating: f64,
    pub rd: f64,
    pub games_played: i32,
    pub games_won: i32,
    pub win_rate: f64,
}

impl From<db::UserRow> for PlayerProfile {
    fn from(u: db::UserRow) -> Self {
        let win_rate = if u.games_played > 0 {
            u.games_won as f64 / u.games_played as f64
        } else {
            0.0
        };
        Self {
            id: u.id,
            username: u.username,
            avatar_url: u.avatar_url,
            rating: u.rating,
            rd: u.rd,
            games_played: u.games_played,
            games_won: u.games_won,
            win_rate,
        }
    }
}

#[derive(Deserialize)]
pub struct LeaderboardQuery {
    pub limit: Option<i64>,
}

/// GET /api/ladder -- top players by rating.
pub async fn leaderboard(
    State(state): State<AppState>,
    Query(query): Query<LeaderboardQuery>,
) -> Result<Json<Vec<PlayerProfile>>, (StatusCode, String)> {
    let pool = state.require_db()?;
    let limit = query.limit.unwrap_or(50);
    let users = db::get_leaderboard(pool, limit)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(users.into_iter().map(PlayerProfile::from).collect()))
}

/// GET /api/users/:id -- player profile.
pub async fn player_profile(
    State(state): State<AppState>,
    Path(user_id): Path<Uuid>,
) -> Result<Json<PlayerProfile>, (StatusCode, String)> {
    let pool = state.require_db()?;
    let user = db::get_user(pool, user_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "user not found".to_string()))?;
    Ok(Json(PlayerProfile::from(user)))
}
