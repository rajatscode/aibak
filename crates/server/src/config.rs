/// Application configuration loaded from environment variables.
#[derive(Debug, Clone)]
pub struct Config {
    /// PostgreSQL connection URL (optional for local play).
    pub database_url: Option<String>,
    /// Discord OAuth2 client ID.
    pub discord_client_id: Option<String>,
    /// Discord OAuth2 client secret.
    pub discord_client_secret: Option<String>,
    /// Discord OAuth2 redirect URI.
    pub discord_redirect_uri: Option<String>,
    /// Secret key for signing JWT tokens.
    pub jwt_secret: String,
    /// Address to bind the server to.
    pub bind_addr: String,
    /// Default map file path for local play.
    pub default_map_path: String,
    /// Turn deadline in seconds (default 5 minutes).
    pub turn_deadline_secs: u64,
}

impl Config {
    /// Load configuration from environment variables.
    /// Falls back to sensible defaults for local play mode.
    pub fn from_env() -> Self {
        Self {
            database_url: std::env::var("DATABASE_URL").ok(),
            discord_client_id: std::env::var("DISCORD_CLIENT_ID").ok(),
            discord_client_secret: std::env::var("DISCORD_CLIENT_SECRET").ok(),
            discord_redirect_uri: std::env::var("DISCORD_REDIRECT_URI").ok(),
            jwt_secret: std::env::var("JWT_SECRET")
                .unwrap_or_else(|_| "dev-secret-change-in-production".to_string()),
            bind_addr: std::env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:3000".to_string()),
            default_map_path: std::env::var("DEFAULT_MAP")
                .unwrap_or_else(|_| "maps/small_earth.json".to_string()),
            turn_deadline_secs: std::env::var("TURN_DEADLINE_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(300),
        }
    }

    /// Returns true if Discord OAuth2 is fully configured.
    pub fn discord_configured(&self) -> bool {
        self.discord_client_id.is_some()
            && self.discord_client_secret.is_some()
            && self.discord_redirect_uri.is_some()
    }

    /// Returns true if a database is configured.
    pub fn db_configured(&self) -> bool {
        self.database_url.is_some()
    }
}
