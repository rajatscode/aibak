use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::db;
use crate::game::league;
use crate::AppState;

// ── Response types ──

#[derive(Serialize)]
pub struct SeasonResponse {
    pub id: i32,
    pub name: String,
    pub starts_at: String,
    pub ends_at: String,
    pub is_active: bool,
}

impl From<db::SeasonRow> for SeasonResponse {
    fn from(s: db::SeasonRow) -> Self {
        Self {
            id: s.id,
            name: s.name,
            starts_at: s.starts_at.to_rfc3339(),
            ends_at: s.ends_at.to_rfc3339(),
            is_active: s.is_active,
        }
    }
}

#[derive(Serialize)]
pub struct StandingResponse {
    pub user_id: Uuid,
    pub username: String,
    pub avatar_url: Option<String>,
    pub rank_tier: String,
    pub rank_points: i32,
    pub rank_color: &'static str,
    pub wins: i32,
    pub losses: i32,
    pub streak: i32,
    pub peak_rank_points: i32,
    pub win_rate: f64,
}

impl From<db::LeaderboardEntry> for StandingResponse {
    fn from(e: db::LeaderboardEntry) -> Self {
        let total = e.wins + e.losses;
        let win_rate = if total > 0 {
            e.wins as f64 / total as f64
        } else {
            0.0
        };
        let tier = league::rank_tier_for_rp(e.rank_points);
        Self {
            user_id: e.user_id,
            username: e.username,
            avatar_url: e.avatar_url,
            rank_tier: e.rank_tier,
            rank_points: e.rank_points,
            rank_color: tier.color,
            wins: e.wins,
            losses: e.losses,
            streak: e.streak,
            peak_rank_points: e.peak_rank_points,
            win_rate,
        }
    }
}

#[derive(Serialize)]
pub struct PlayerSeasonStats {
    pub season: SeasonResponse,
    pub standing: StandingResponse,
}

#[derive(Serialize)]
pub struct MatchHistoryResponse {
    pub id: Uuid,
    pub game_id: Uuid,
    pub season_id: Option<i32>,
    pub player_a: Uuid,
    pub player_b: Uuid,
    pub winner_id: Option<Uuid>,
    pub player_a_rating_change: Option<f64>,
    pub player_b_rating_change: Option<f64>,
    pub player_a_rp_change: Option<i32>,
    pub player_b_rp_change: Option<i32>,
    pub turns_played: Option<i32>,
    pub template: Option<String>,
    pub played_at: String,
}

impl From<db::MatchHistoryRow> for MatchHistoryResponse {
    fn from(m: db::MatchHistoryRow) -> Self {
        Self {
            id: m.id,
            game_id: m.game_id,
            season_id: m.season_id,
            player_a: m.player_a,
            player_b: m.player_b,
            winner_id: m.winner_id,
            player_a_rating_change: m.player_a_rating_change,
            player_b_rating_change: m.player_b_rating_change,
            player_a_rp_change: m.player_a_rp_change,
            player_b_rp_change: m.player_b_rp_change,
            turns_played: m.turns_played,
            template: m.template,
            played_at: m.played_at.to_rfc3339(),
        }
    }
}

// ── Query params ──

#[derive(Deserialize)]
pub struct LeaderboardQuery {
    pub limit: Option<i64>,
}

#[derive(Deserialize)]
pub struct MatchHistoryQuery {
    pub user_id: Option<Uuid>,
    pub season_id: Option<i32>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

// ── Handlers ──

/// GET /api/seasons -- list all seasons.
pub async fn list_seasons(
    State(state): State<AppState>,
) -> Result<Json<Vec<SeasonResponse>>, (StatusCode, String)> {
    let pool = state.require_db()?;
    let seasons = db::list_seasons(pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(seasons.into_iter().map(SeasonResponse::from).collect()))
}

/// GET /api/seasons/current -- get the active season.
pub async fn current_season(
    State(state): State<AppState>,
) -> Result<Json<Option<SeasonResponse>>, (StatusCode, String)> {
    let pool = state.require_db()?;
    let season = db::get_active_season(pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(season.map(SeasonResponse::from)))
}

/// GET /api/seasons/:id/standings -- leaderboard for a season.
pub async fn season_standings(
    State(state): State<AppState>,
    Path(season_id): Path<i32>,
    Query(query): Query<LeaderboardQuery>,
) -> Result<Json<Vec<StandingResponse>>, (StatusCode, String)> {
    let pool = state.require_db()?;
    let limit = query.limit.unwrap_or(50);
    let entries = db::get_season_leaderboard(pool, season_id, limit)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(entries.into_iter().map(StandingResponse::from).collect()))
}

/// GET /api/seasons/:season_id/standings/:user_id -- player's season stats.
pub async fn player_season_stats(
    State(state): State<AppState>,
    Path((season_id, user_id)): Path<(i32, Uuid)>,
) -> Result<Json<PlayerSeasonStats>, (StatusCode, String)> {
    let pool = state.require_db()?;

    let season = db::get_season(pool, season_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "season not found".to_string()))?;

    let standing = db::get_or_create_standing(pool, season_id, user_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Convert standing row to a LeaderboardEntry-like structure for the response.
    let user = db::get_user(pool, user_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "user not found".to_string()))?;

    let entry = db::LeaderboardEntry {
        user_id: standing.user_id,
        username: user.username,
        avatar_url: user.avatar_url,
        rank_tier: standing.rank_tier,
        rank_points: standing.rank_points,
        wins: standing.wins,
        losses: standing.losses,
        streak: standing.streak,
        peak_rank_points: standing.peak_rank_points,
    };

    Ok(Json(PlayerSeasonStats {
        season: SeasonResponse::from(season),
        standing: StandingResponse::from(entry),
    }))
}

/// GET /api/match-history -- paginated match history.
pub async fn match_history(
    State(state): State<AppState>,
    Query(query): Query<MatchHistoryQuery>,
) -> Result<Json<Vec<MatchHistoryResponse>>, (StatusCode, String)> {
    let pool = state.require_db()?;
    let limit = query.limit.unwrap_or(20).min(100);
    let offset = query.offset.unwrap_or(0);
    let matches = db::get_match_history(pool, query.user_id, query.season_id, limit, offset)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(matches.into_iter().map(MatchHistoryResponse::from).collect()))
}
