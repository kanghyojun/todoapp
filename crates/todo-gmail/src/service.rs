use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;
use tokio::sync::{RwLock, broadcast};
use todo_core::TodoCore;

use crate::error::Error;
use crate::model::{GmailAccount, MailEvent};
use crate::tokens::TokenStore;
use crate::{oauth, store};

const DEFAULT_OAUTH_BASE: &str = "https://oauth2.googleapis.com";
const DEFAULT_GMAIL_BASE: &str = "https://gmail.googleapis.com/gmail/v1";

#[derive(Clone)]
pub struct GmailService {
    core: TodoCore,
    client: reqwest::Client,
    tokens: Arc<dyn TokenStore>,
    oauth_base: String,
    gmail_base: String,
    access_cache: Arc<RwLock<HashMap<String, (String, i64)>>>,
    events: broadcast::Sender<MailEvent>,
}

impl GmailService {
    pub fn new(core: TodoCore, tokens: Arc<dyn TokenStore>) -> Self {
        Self::with_endpoints(core, tokens, DEFAULT_OAUTH_BASE, DEFAULT_GMAIL_BASE)
    }

    pub fn with_endpoints(
        core: TodoCore,
        tokens: Arc<dyn TokenStore>,
        oauth_base: impl Into<String>,
        gmail_base: impl Into<String>,
    ) -> Self {
        let (events, _) = broadcast::channel(64);
        Self {
            core,
            client: reqwest::Client::new(),
            tokens,
            oauth_base: oauth_base.into(),
            gmail_base: gmail_base.into(),
            access_cache: Arc::new(RwLock::new(HashMap::new())),
            events,
        }
    }

    pub async fn set_client_credentials(
        &self,
        client_id: &str,
        client_secret: &str,
    ) -> Result<(), Error> {
        if client_id.trim().is_empty() || client_secret.trim().is_empty() {
            return Err(Error::InvalidInput(
                "client id and secret must not be blank".to_owned(),
            ));
        }
        store::set_setting(self.core.pool(), "gmail.client_id", client_id).await?;
        store::set_setting(self.core.pool(), "gmail.client_secret", client_secret).await?;
        Ok(())
    }

    pub async fn client_id(&self) -> Result<Option<String>, Error> {
        store::get_setting(self.core.pool(), "gmail.client_id").await
    }

    /// OAuth 콜백에서 받은 code 를 토큰으로 교환하고 계정을 등록한다.
    pub async fn complete_auth(
        &self,
        code: &str,
        verifier: &str,
        redirect_uri: &str,
    ) -> Result<GmailAccount, Error> {
        let (client_id, client_secret) = self.require_credentials().await?;
        let token = oauth::exchange_code(
            &self.client,
            &self.oauth_base,
            &client_id,
            &client_secret,
            code,
            verifier,
            redirect_uri,
        )
        .await?;
        let refresh = token.refresh_token.clone().ok_or_else(|| {
            Error::InvalidInput(
                "Google did not return a refresh token; retry with consent".to_owned(),
            )
        })?;
        let profile =
            oauth::fetch_profile(&self.client, &self.gmail_base, &token.access_token).await?;
        self.tokens.set(&profile.email, &refresh)?;
        let account = store::insert_account(self.core.pool(), &profile.email).await?;
        store::set_history_id(self.core.pool(), &account.id, &profile.history_id).await?;
        self.cache_access_token(&profile.email, &token.access_token, token.expires_in)
            .await;
        self.emit();
        store::fetch_account(self.core.pool(), &account.id).await
    }

    async fn require_credentials(&self) -> Result<(String, String), Error> {
        let id = store::get_setting(self.core.pool(), "gmail.client_id")
            .await?
            .ok_or(Error::NotConfigured)?;
        let secret = store::get_setting(self.core.pool(), "gmail.client_secret")
            .await?
            .ok_or(Error::NotConfigured)?;
        Ok((id, secret))
    }

    async fn cache_access_token(&self, email: &str, token: &str, expires_in: i64) {
        let expiry = Utc::now().timestamp() + expires_in - 60;
        self.access_cache
            .write()
            .await
            .insert(email.to_owned(), (token.to_owned(), expiry));
    }

    fn emit(&self) {
        let _ = self.events.send(MailEvent::Changed);
    }
}
