use serde::{Deserialize, Serialize};

const DISCORD_AUTH_URL: &str = "https://discord.com/api/oauth2/authorize";
const DISCORD_TOKEN_URL: &str = "https://discord.com/api/oauth2/token";
const DISCORD_USER_URL: &str = "https://discord.com/api/users/@me";

/// Discord user info returned from their API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordUser {
    pub id: String,
    pub username: String,
    pub avatar: Option<String>,
    pub discriminator: String,
}

impl DiscordUser {
    /// Build the avatar URL for this user.
    pub fn avatar_url(&self) -> Option<String> {
        self.avatar.as_ref().map(|hash| {
            format!(
                "https://cdn.discordapp.com/avatars/{}/{}.png",
                self.id, hash
            )
        })
    }

    /// Parse the Discord ID string into an i64.
    pub fn discord_id_i64(&self) -> Result<i64, std::num::ParseIntError> {
        self.id.parse()
    }
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[allow(dead_code)]
    token_type: String,
}

/// Build the Discord authorization URL for the OAuth2 code grant flow.
pub fn build_auth_url(client_id: &str, redirect_uri: &str) -> String {
    format!(
        "{}?client_id={}&redirect_uri={}&response_type=code&scope=identify",
        DISCORD_AUTH_URL,
        client_id,
        urlencoding::encode(redirect_uri)
    )
}

/// Exchange an authorization code for an access token.
pub async fn exchange_code(
    client_id: &str,
    client_secret: &str,
    redirect_uri: &str,
    code: &str,
) -> Result<String, reqwest::Error> {
    let client = reqwest::Client::new();
    let resp = client
        .post(DISCORD_TOKEN_URL)
        .form(&[
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
        ])
        .send()
        .await?
        .json::<TokenResponse>()
        .await?;
    Ok(resp.access_token)
}

/// Fetch the current user's info from Discord using an access token.
pub async fn fetch_user(access_token: &str) -> Result<DiscordUser, reqwest::Error> {
    let client = reqwest::Client::new();
    client
        .get(DISCORD_USER_URL)
        .bearer_auth(access_token)
        .send()
        .await?
        .json::<DiscordUser>()
        .await
}

/// Simple percent-encoding for URLs (only encodes what Discord needs).
mod urlencoding {
    pub fn encode(input: &str) -> String {
        let mut result = String::with_capacity(input.len() * 3);
        for byte in input.bytes() {
            match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    result.push(byte as char);
                }
                _ => {
                    result.push('%');
                    result.push_str(&format!("{:02X}", byte));
                }
            }
        }
        result
    }
}
