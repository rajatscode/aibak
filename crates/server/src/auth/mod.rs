pub mod discord;
pub mod session;

use axum::{
    extract::{FromRequestParts, Query},
    http::{request::Parts, StatusCode},
};
use serde::Deserialize;
use uuid::Uuid;

use crate::AppState;

/// Authenticated user extracted from JWT token.
/// Checks: Authorization header (Bearer), cookie (token), or query param (?token=).
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user_id: Uuid,
    pub username: String,
}

#[derive(Deserialize)]
struct TokenQuery {
    token: Option<String>,
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let jwt_secret = &state.config.jwt_secret;

        // Try Authorization header first.
        if let Some(auth_header) = parts.headers.get("authorization") {
            if let Ok(header_str) = auth_header.to_str() {
                if let Some(token) = header_str.strip_prefix("Bearer ") {
                    if let Ok(claims) = session::verify_token(token, jwt_secret) {
                        return Ok(AuthUser {
                            user_id: claims.sub,
                            username: claims.username,
                        });
                    }
                }
            }
        }

        // Try cookie.
        if let Some(cookie_header) = parts.headers.get("cookie") {
            if let Ok(cookie_str) = cookie_header.to_str() {
                for cookie in cookie_str.split(';') {
                    let cookie = cookie.trim();
                    if let Some(token) = cookie.strip_prefix("token=") {
                        if let Ok(claims) = session::verify_token(token, jwt_secret) {
                            return Ok(AuthUser {
                                user_id: claims.sub,
                                username: claims.username,
                            });
                        }
                    }
                }
            }
        }

        // Try query parameter (used for WebSocket connections).
        if let Ok(Query(q)) = Query::<TokenQuery>::try_from_uri(&parts.uri) {
            if let Some(token) = q.token {
                if let Ok(claims) = session::verify_token(&token, jwt_secret) {
                    return Ok(AuthUser {
                        user_id: claims.sub,
                        username: claims.username,
                    });
                }
            }
        }

        Err((StatusCode::UNAUTHORIZED, "missing or invalid authentication"))
    }
}

/// Optional auth extractor: does not reject if unauthenticated.
#[derive(Debug, Clone)]
pub struct MaybeAuthUser(pub Option<AuthUser>);

impl FromRequestParts<AppState> for MaybeAuthUser {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        match AuthUser::from_request_parts(parts, state).await {
            Ok(user) => Ok(MaybeAuthUser(Some(user))),
            Err(_) => Ok(MaybeAuthUser(None)),
        }
    }
}
