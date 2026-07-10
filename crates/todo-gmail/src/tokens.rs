use crate::error::TokenStoreError;

const KEYRING_SERVICE: &str = "todo";

/// 계정 이메일별로 refresh 토큰을 보관하는 저장소.
pub trait TokenStore: Send + Sync {
    fn get(&self, email: &str) -> Result<Option<String>, TokenStoreError>;
    fn set(&self, email: &str, refresh_token: &str) -> Result<(), TokenStoreError>;
    fn delete(&self, email: &str) -> Result<(), TokenStoreError>;
}

#[derive(Debug, Default)]
pub struct SystemTokenStore;

impl SystemTokenStore {
    fn entry(email: &str) -> Result<keyring::Entry, TokenStoreError> {
        keyring::Entry::new(KEYRING_SERVICE, &format!("gmail-refresh:{email}"))
            .map_err(|_| TokenStoreError)
    }
}

impl TokenStore for SystemTokenStore {
    fn get(&self, email: &str) -> Result<Option<String>, TokenStoreError> {
        match Self::entry(email)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(TokenStoreError),
        }
    }

    fn set(&self, email: &str, refresh_token: &str) -> Result<(), TokenStoreError> {
        Self::entry(email)?
            .set_password(refresh_token)
            .map_err(|_| TokenStoreError)
    }

    fn delete(&self, email: &str) -> Result<(), TokenStoreError> {
        match Self::entry(email)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(TokenStoreError),
        }
    }
}
