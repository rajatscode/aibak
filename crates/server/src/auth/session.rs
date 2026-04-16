use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Claims embedded in JWT tokens.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    /// User ID.
    pub sub: Uuid,
    /// Username (for display convenience).
    pub username: String,
    /// Expiration timestamp (seconds since epoch).
    pub exp: i64,
    /// Issued at timestamp.
    pub iat: i64,
}

/// Create a signed JWT token for a user.
pub fn create_token(
    user_id: Uuid,
    username: &str,
    secret: &str,
) -> Result<String, jsonwebtoken::errors::Error> {
    let now = Utc::now();
    let exp = now + Duration::days(7);
    let claims = Claims {
        sub: user_id,
        username: username.to_string(),
        exp: exp.timestamp(),
        iat: now.timestamp(),
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
}

/// Validate and decode a JWT token.
pub fn verify_token(token: &str, secret: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
    let data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )?;
    Ok(data.claims)
}
