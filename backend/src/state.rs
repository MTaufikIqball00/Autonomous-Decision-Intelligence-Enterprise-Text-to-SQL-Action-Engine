use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::{Duration, Instant},
};

use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

use crate::{
    auth::JwtService,
    config::AppConfig,
    crypto::SecretCipher,
    db::{ConnectionParams, PostgresAdapter},
    error::AppError,
};

#[derive(Clone)]
pub struct CachedPool {
    pub pool: PgPool,
    pub fingerprint: String,
}

#[derive(Clone, Default)]
pub struct SlidingWindowRateLimiter {
    buckets: Arc<Mutex<HashMap<String, VecDeque<Instant>>>>,
}

impl SlidingWindowRateLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn check_and_record(
        &self,
        key: &str,
        max_requests: u32,
        window: Duration,
    ) -> Result<(), u64> {
        if max_requests == 0 {
            return Ok(());
        }
        let now = Instant::now();
        let mut buckets = self.buckets.lock().await;
        // Prune stale empty keys periodically to keep memory bounded.
        if buckets.len() > 2048 {
            buckets.retain(|_, q| {
                while let Some(&front) = q.front() {
                    if now.duration_since(front) >= window {
                        q.pop_front();
                    } else {
                        break;
                    }
                }
                !q.is_empty()
            });
        }

        let queue = buckets.entry(key.to_string()).or_default();
        while let Some(&front) = queue.front() {
            if now.duration_since(front) >= window {
                queue.pop_front();
            } else {
                break;
            }
        }

        if queue.len() >= max_requests as usize {
            let retry_after = queue
                .front()
                .map(|&oldest| {
                    let elapsed = now.duration_since(oldest);
                    window.saturating_sub(elapsed).as_secs().max(1)
                })
                .unwrap_or(60);
            return Err(retry_after);
        }

        queue.push_back(now);
        Ok(())
    }
}

#[derive(Clone)]
pub struct AppState {
    pub config: AppConfig,
    pub app_pool: PgPool,
    pub jwt: JwtService,
    pub cipher: SecretCipher,
    pub analytics_pools: Arc<RwLock<HashMap<Uuid, CachedPool>>>,
    pub rate_limiter: SlidingWindowRateLimiter,
}

impl AppState {
    pub fn new(
        config: AppConfig,
        app_pool: PgPool,
        jwt: JwtService,
        cipher: SecretCipher,
    ) -> Self {
        Self {
            config,
            app_pool,
            jwt,
            cipher,
            analytics_pools: Arc::new(RwLock::new(HashMap::new())),
            rate_limiter: SlidingWindowRateLimiter::new(),
        }
    }

    /// Returns a cached PostgreSQL adapter for the target connection, creating and caching
    /// the underlying `PgPool` when absent or when connection parameters have changed.
    pub async fn analytics_adapter(&self, params: &ConnectionParams) -> Result<PostgresAdapter, AppError> {
        let fingerprint = connection_fingerprint(params);
        {
            let guard = self.analytics_pools.read().await;
            if let Some(entry) = guard.get(&params.id) {
                if entry.fingerprint == fingerprint && !entry.pool.is_closed() {
                    return Ok(PostgresAdapter::from_pool(entry.pool.clone(), params.clone()));
                }
            }
        }

        let adapter = PostgresAdapter::connect(params.clone()).await?;
        let mut guard = self.analytics_pools.write().await;
        // Double-check in case another task inserted while we were connecting.
        if let Some(existing) = guard.get(&params.id) {
            if existing.fingerprint == fingerprint && !existing.pool.is_closed() {
                let pool = existing.pool.clone();
                drop(guard);
                adapter.pool().close().await;
                return Ok(PostgresAdapter::from_pool(pool, params.clone()));
            }
        }
        guard.insert(
            params.id,
            CachedPool {
                pool: adapter.pool().clone(),
                fingerprint,
            },
        );
        Ok(adapter)
    }

    /// Invalidates and closes any cached analytics pool for `connection_id`.
    pub async fn invalidate_analytics_pool(&self, connection_id: Uuid) {
        let removed = {
            let mut guard = self.analytics_pools.write().await;
            guard.remove(&connection_id)
        };
        if let Some(entry) = removed {
            entry.pool.close().await;
        }
    }
}

fn connection_fingerprint(params: &ConnectionParams) -> String {
    let mut hasher = Sha256::new();
    hasher.update(params.adapter_type.as_bytes());
    hasher.update(b":");
    hasher.update(params.host.as_bytes());
    hasher.update(b":");
    hasher.update(params.port.to_le_bytes());
    hasher.update(b":");
    hasher.update(params.database_name.as_bytes());
    hasher.update(b":");
    hasher.update(params.username.as_bytes());
    hasher.update(b":");
    hasher.update(params.password.as_bytes());
    hasher.update(b":");
    hasher.update(params.ssl_mode.as_bytes());
    hex::encode(hasher.finalize())
}
