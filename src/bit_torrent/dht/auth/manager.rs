use std::{collections::HashMap, net::IpAddr, sync::Arc, time::Duration};

use notify::{EventKind, RecursiveMode, Watcher, recommended_watcher};
use pgp::{
    composed::{DetachedSignature, SignedPublicKey},
    ser::Serialize,
};
use tokio::{sync::RwLock, time::sleep};

use crate::{
    bit_torrent::certs::PublicKey,
    certs::{ActiveKeyIdentity, CertError, KeyIdentity},
    dht::{
        Key, Manifest, Node,
        auth::{
            ActiveProver, AuthError, Authorizable, ChallangeProof, Evidence, PoW, SecretSalt,
            Token, TrustLevel, make_prover,
        },
    },
    types::{Hash32Bytes, UnixDate},
    utils::{
        bqti::{fetch_current_timestamp, inner_files, swarm_dir},
        certs::{load_pgp_keys, load_pgp_signagure},
    },
};

const SECRET_SALT_REFRESH_DURATION: Duration = Duration::from_secs(5);

const REQUEST_NUMBER: Requests = 100;
const REQUEST_PER_SECOND: UnixDate = 60;

type PGPKeys = RwLock<HashMap<[u8; 32], SignedPublicKey>>;

pub struct AuthManager {
    ca_root_certificate: Arc<ActiveKeyIdentity>,
    pgp_details: Option<(Hash32Bytes, DetachedSignature)>,
    pub pgp_keys: Arc<PGPKeys>,
    inner_cert: Arc<ActiveKeyIdentity>,
    prover: Arc<ActiveProver>,
    secret_salt: RwLock<SecretSalt>,
    rate_limiter: RateLimiter,
    token: RwLock<Option<Token>>,
}

impl AuthManager {
    pub fn new(ca_root: Arc<ActiveKeyIdentity>, cert_name: &str) -> Result<Arc<Self>, CertError> {
        let leaf_certificate = ca_root.leaf(cert_name, false)?;

        let pgp_signature = load_pgp_signagure()
            .map_err(|_| {
                warn!("PGP signature not found, this node can't serve as bootstrap");
            })
            .ok();

        let certificate = Arc::new(leaf_certificate);
        let prover = make_prover(certificate.clone());
        let pgp_keys = Arc::new(RwLock::new(HashMap::new()));

        let auth_manager = Arc::new(Self {
            inner_cert: certificate,
            secret_salt: RwLock::new(SecretSalt::new()),
            rate_limiter: RateLimiter::new(REQUEST_NUMBER, REQUEST_PER_SECOND),
            prover: Arc::new(prover),
            ca_root_certificate: ca_root,
            pgp_details: pgp_signature,
            pgp_keys: pgp_keys.clone(),
            token: RwLock::new(None),
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

        watch_pgp_keys_directory(pgp_keys);

        Ok(auth_manager)
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
                    TrustLevel::Attested
                } else {
                    TrustLevel::Rejected
                }
            }
            None => TrustLevel::Unattested,
        };

        let mut token = Token::new(sender.id.pub_key(), secret.value, trust_level);

        if let Some((swarm_id, pgp_signature)) = &self.pgp_details {
            let Ok(pgp_sig_bytes) = pgp_signature.to_bytes() else {
                return Err(AuthError::InvalidPGPSignature());
            };

            token.bind_swarm(swarm_id, &self.ca_root_certificate, &pgp_sig_bytes);
        }

        token.sign(&*self.inner_cert)?;

        Ok(token)
    }

    pub async fn store_token(&self, token: Token) {
        *self.token.write().await = Some(token);
    }

    pub async fn best_token(&self) -> Option<Token> {
        self.token
            .read()
            .await
            .as_ref()
            .filter(|t| !t.is_expired())
            .cloned()
    }

    pub fn certificate(&self) -> &ActiveKeyIdentity {
        &self.inner_cert
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
        self.inner_cert.pub_key()
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

fn watch_pgp_keys_directory(pgp_keys: Arc<PGPKeys>) {
    let Some(swarm_dir) = swarm_dir() else {
        warn!("swarm directory not found, unable to load pgp swarm keys");
        return;
    };

    std::thread::spawn(move || {
        let (watch_tx, watch_rx) = std::sync::mpsc::channel();

        let Ok(mut watcher) = recommended_watcher(watch_tx) else {
            warn!("failed to watch swarm directory");
            return;
        };

        if let Err(_) = watcher.watch(&swarm_dir, RecursiveMode::NonRecursive) {
            error!("failed to start background watching process");
            return;
        }

        inner_files(&swarm_dir)
            .and_then(|paths| load_pgp_keys(paths).ok())
            .map(|keys| {
                let count = keys.len();
                pgp_keys.blocking_write().extend(keys);

                info!("loaded {} pgp swarm key(s)", count);
            });

        while let Ok(result) = watch_rx.recv() {
            let event = match result {
                Ok(event) => event,
                Err(_) => continue,
            };

            match event.kind {
                EventKind::Create(_) | EventKind::Modify(_) => {
                    match load_pgp_keys(event.paths) {
                        Ok(k) if !k.is_empty() => {
                            let count = k.len();
                            pgp_keys.blocking_write().extend(k);
                            info!("{} pgp swarm key(s) loaded", count);
                        }
                        Ok(_) => warn!("no pgp keys were found"),
                        Err(_) => warn!("failed to load pgp keys"),
                    };
                }

                EventKind::Remove(_) => {
                    match inner_files(&swarm_dir).and_then(|paths| load_pgp_keys(paths).ok()) {
                        Some(k) if !k.is_empty() => {
                            let count = k.len();
                            let mut keys = pgp_keys.blocking_write();

                            keys.clear();
                            keys.extend(k);

                            info!("pgp keys reloaded: {} key(s) active", count);
                        }
                        _ => {
                            pgp_keys.blocking_write().clear();

                            warn!(
                                "all pgp swarm keys removed, node can no longer interact with any bootstrap"
                            );
                        }
                    }
                }
                _ => (),
            }
        }
    });
}
