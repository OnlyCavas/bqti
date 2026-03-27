use std::net::IpAddr;

use tokio::sync::RwLock;

use crate::{
    bit_torrent::certs::{KeyIdentity, PublicKey},
    dht::{
        Node,
        auth::{AuthError, Authorizable, ChallangeProof, Evidence, PoW, SecretSalt, Token},
    },
};

pub struct AuthManager {
    certificate: KeyIdentity,
    secret_salt: RwLock<SecretSalt>, // NOTE implement cycle event

    tokens: RwLock<Vec<Token>>,
}

impl AuthManager {
    pub fn new(certificate: KeyIdentity) -> Self {
        Self {
            certificate,
            secret_salt: RwLock::new(SecretSalt::new()),
            tokens: RwLock::new(Vec::new()),
        }

        // NOTE return weak reference to start
    }

    pub async fn challange(&self, pub_key: &[u8], ip: &IpAddr) -> u32 {
        let secret_salt = {
            let secret = self.secret_salt.read().await;
            *secret
        };

        SecretSalt::calculate_challenge(&pub_key, &ip, &secret_salt)
    }

    pub async fn issue_token(&self, sender: &Node, secret: &PoW) -> Result<Token, AuthError> {
        if !secret.verify(sender.id.pub_key()) {
            return Err(AuthError::RoguePeer());
        }

        let mut token = Token::new(sender.id.pub_key(), secret.value);
        token.sign(&self.certificate)?;

        Ok(token)
    }

    pub async fn store_token(&self, token: Token) {
        let mut held_tokens = self.tokens.write().await;
        held_tokens.push(token);
    }

    pub async fn best_token(&self) -> Option<Token> {
        let held = self.tokens.read().await;
        held.iter().find(|t| !t.is_expired()).cloned()
    }

    pub fn certificate(&self) -> &KeyIdentity {
        &self.certificate
    }
}

impl PublicKey for AuthManager {
    fn pub_key(&self) -> &[u8] {
        self.certificate.pub_key()
    }
}
