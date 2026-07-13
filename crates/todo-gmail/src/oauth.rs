use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::RngCore;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::error::Error;

const AUTH_ENDPOINT: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const SCOPE: &str = "https://www.googleapis.com/auth/gmail.modify";

fn random_token(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::thread_rng().fill_bytes(&mut buf);
    URL_SAFE_NO_PAD.encode(buf)
}

pub fn random_state() -> String {
    random_token(16)
}

/// PKCE (verifier, challenge) 쌍. challenge = base64url(sha256(verifier)).
pub fn generate_pkce() -> (String, String) {
    let verifier = random_token(48);
    let digest = Sha256::digest(verifier.as_bytes());
    let challenge = URL_SAFE_NO_PAD.encode(digest);
    (verifier, challenge)
}

pub fn build_auth_url(client_id: &str, redirect_uri: &str, challenge: &str, state: &str) -> String {
    let params = [
        ("client_id", client_id),
        ("redirect_uri", redirect_uri),
        ("response_type", "code"),
        ("scope", SCOPE),
        ("access_type", "offline"),
        ("prompt", "consent"),
        ("code_challenge", challenge),
        ("code_challenge_method", "S256"),
        ("state", state),
    ];
    let query = params
        .iter()
        .map(|(key, value)| format!("{key}={}", urlencode(value)))
        .collect::<Vec<_>>()
        .join("&");
    format!("{AUTH_ENDPOINT}?{query}")
}

fn urlencode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

#[derive(Debug, Deserialize)]
pub(crate) struct TokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    pub expires_in: i64,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Profile {
    #[serde(rename = "emailAddress")]
    pub email: String,
    #[serde(rename = "historyId")]
    pub history_id: String,
}

pub(crate) async fn exchange_code(
    client: &reqwest::Client,
    oauth_base: &str,
    client_id: &str,
    client_secret: &str,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> Result<TokenResponse, Error> {
    post_token(
        client,
        oauth_base,
        &[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("code_verifier", verifier),
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("redirect_uri", redirect_uri),
        ],
    )
    .await
}

pub(crate) async fn refresh_token(
    client: &reqwest::Client,
    oauth_base: &str,
    client_id: &str,
    client_secret: &str,
    refresh_token: &str,
) -> Result<TokenResponse, Error> {
    post_token(
        client,
        oauth_base,
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", client_id),
            ("client_secret", client_secret),
        ],
    )
    .await
}

async fn post_token(
    client: &reqwest::Client,
    oauth_base: &str,
    form: &[(&str, &str)],
) -> Result<TokenResponse, Error> {
    let body = form
        .iter()
        .map(|(key, value)| format!("{}={}", urlencode(key), urlencode(value)))
        .collect::<Vec<_>>()
        .join("&");
    let response = client
        .post(format!("{oauth_base}/token"))
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body(body)
        .send()
        .await
        .map_err(|_| Error::Remote {
            message: "could not reach Google OAuth".to_owned(),
            retryable: true,
        })?;
    let status = response.status();
    if status == reqwest::StatusCode::BAD_REQUEST || status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(Error::Unauthorized);
    }
    if !status.is_success() {
        return Err(Error::Remote {
            message: format!("token endpoint returned HTTP {status}"),
            retryable: status.is_server_error(),
        });
    }
    response.json().await.map_err(|_| Error::Remote {
        message: "token endpoint returned an invalid response".to_owned(),
        retryable: true,
    })
}

pub(crate) async fn fetch_profile(
    client: &reqwest::Client,
    gmail_base: &str,
    access_token: &str,
) -> Result<Profile, Error> {
    let response = client
        .get(format!("{gmail_base}/users/me/profile"))
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|_| Error::Remote {
            message: "could not reach Gmail".to_owned(),
            retryable: true,
        })?;
    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(Error::Unauthorized);
    }
    if !status.is_success() {
        return Err(Error::Remote {
            message: format!("profile returned HTTP {status}"),
            retryable: status.is_server_error(),
        });
    }
    response.json().await.map_err(|_| Error::Remote {
        message: "profile returned an invalid response".to_owned(),
        retryable: true,
    })
}
