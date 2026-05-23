use std::{collections::HashMap, net::IpAddr, sync::Arc, time::Duration};

use tokio::{sync::RwLock, time::sleep};

use crate::{
    bit_torrent::certs::PublicKey,
    certs::ActiveKeyIdentity,
    dht::{
        Key, Manifest, Node,
        auth::{
            ActiveProver, AuthError, Authorizable, ChallangeProof, Evidence, PoW, SecretSalt,
            Token, TrustLevel, make_prover,
        },
    },
    types::UnixDate,
    utils::bqti::fetch_current_timestamp,
};

const SECRET_SALT_REFRESH_DURATION: Duration = Duration::from_secs(5);

const REQUEST_NUMBER: Requests = 100;
const REQUEST_PER_SECOND: UnixDate = 60;

pub struct AuthManager {
    certificate: Arc<ActiveKeyIdentity>,
    prover: Arc<ActiveProver>,
    secret_salt: RwLock<SecretSalt>,
    rate_limiter: RateLimiter,
    tokens: RwLock<Vec<Token>>,
}

impl AuthManager {
    pub fn new(certificate: ActiveKeyIdentity) -> Arc<Self> {
        let certificate = Arc::new(certificate);
        let prover = make_prover(certificate.clone());

        let auth_manager = Arc::new(Self {
            certificate,
            secret_salt: RwLock::new(SecretSalt::new()),
            rate_limiter: RateLimiter::new(REQUEST_NUMBER, REQUEST_PER_SECOND),
            tokens: RwLock::new(Vec::new()),
            prover: Arc::new(prover),
        });

        let weak_ptr = Arc::downgrade(&auth_manager);
        tokio::spawn(async move {
            loop {
                sleep(SECRET_SALT_REFRESH_DURATION).await;

                let Some(manager) = weak_ptr.upgrade() else {
                    return;
                };

                *manager.secret_salt.write().await = SecretSalt::new();
            }
        });

        auth_manager
    }

    pub async fn challange(&self, pub_key: &[u8], ip: &IpAddr) -> u32 {
        let secret_salt = {
            let secret = self.secret_salt.read().await;
            *secret
        };

        SecretSalt::calculate_challenge(&pub_key, &ip, &secret_salt)
    }

    pub async fn issue_token(
        &self,
        sender: &Node,
        secret: &PoW,
        app_version: &str,
    ) -> Result<Token, AuthError> {
        if !secret.verify(sender.id.pub_key()) {
            return Err(AuthError::RoguePeer());
        }

        let trust_level = match &secret.attestation {
            Some(report) => {
                let expected_enclave_hash = Manifest::get_enclave_hash(app_version).await?;

                if report.verify(&secret.value, Some(&expected_enclave_hash)) {
                    TrustLevel::Attested(report.clone())
                } else {
                    TrustLevel::Rejected
                }
            }
            None => TrustLevel::Unattested,
        };

        let mut token = Token::new(sender.id.pub_key(), secret.value, trust_level);
        token.sign(&*self.certificate)?;

        Ok(token)
    }

    pub async fn store_token(&self, token: Token) {
        let mut held_tokens = self.tokens.write().await;
        held_tokens.retain(|t| !t.is_expired());

        if let Some(existing) = held_tokens.iter_mut().find(|t| t.issuer == token.issuer) {
            *existing = token;
        } else {
            held_tokens.push(token);
        }
    }

    pub async fn best_token(&self) -> Option<Token> {
        let held = self.tokens.read().await;

        held.iter()
            .filter(|t| !t.is_expired())
            .max_by_key(|t| match t.trust_level() {
                TrustLevel::Attested(_) => 2,
                TrustLevel::Unattested => 1,
                TrustLevel::Rejected => 0,
            })
            .cloned()
    }

    pub fn certificate(&self) -> &ActiveKeyIdentity {
        &self.certificate
    }

    pub fn prover(&self) -> Arc<ActiveProver> {
        self.prover.clone()
    }

    pub async fn check_rate(&self, peer: &Key) -> Result<(), AuthError> {
        if !self.rate_limiter.check(peer).await {
            return Err(AuthError::RateLimited());
        }

        Ok(())
    }
}

impl PublicKey for AuthManager {
    fn pub_key(&self) -> &[u8] {
        self.certificate.pub_key()
    }
}

type Requests = u32;
type Window = (Requests, UnixDate);

pub struct RateLimiter {
    requests: RwLock<HashMap<Key, Window>>,
    max_per_window: u32,
    window_seconds: UnixDate,
}

impl RateLimiter {
    pub fn new(max_per_window: Requests, window_seconds: UnixDate) -> Self {
        Self {
            requests: RwLock::new(HashMap::new()),
            max_per_window,
            window_seconds,
        }
    }

    pub async fn check(&self, sender_key: &Key) -> bool {
        let now = fetch_current_timestamp();
        let mut requests = self.requests.write().await;

        requests.retain(|_, (_, start)| now - *start <= self.window_seconds);

        let (count, start) = requests.entry(sender_key.clone()).or_insert((0, now));

        if now - *start > self.window_seconds {
            *count = 1;
            *start = now;
            return true;
        }

        if *count >= self.max_per_window {
            return false;
        }

        *count += 1;
        true
    }
}
