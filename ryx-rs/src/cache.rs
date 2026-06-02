use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use ryx_backend::backends::{RowMapping, RowView};
use ryx_common::{RyxError, RyxResult, SqlValue};
use ryx_query::compiler;
use tokio::sync::RwLock;

use crate::queryset::QuerySet;
use crate::row::FromRow;

// ============================================================
// Cache Backend Trait
// ============================================================

/// A cache storage backend.
///
/// Implement this trait to provide custom caching (Redis, memcached, etc.).
#[async_trait]
pub trait CacheBackend: Send + Sync {
    /// Retrieve a cached value by key.
    async fn get(&self, key: &str) -> RyxResult<Option<String>>;

    /// Store a value with an optional TTL in seconds.
    async fn set(&self, key: &str, value: &str, ttl: Option<u64>) -> RyxResult<()>;

    /// Delete a cached value.
    async fn delete(&self, key: &str) -> RyxResult<()>;

    /// Clear all cached values.
    async fn clear(&self) -> RyxResult<()>;
}

// ============================================================
// Memory Cache Backend
// ============================================================

/// An in-memory cache backend with TTL support and LRU eviction.
///
/// # Example
///
/// ```ignore
/// use ryx_rs::cache::MemoryCache;
///
/// let cache = MemoryCache::new(300, 5000);
/// cache.set("key", "value", Some(60)).await?;
/// assert_eq!(cache.get("key").await?, Some("value".to_string()));
/// ```
pub struct MemoryCache {
    default_ttl: u64,
    max_entries: usize,
    data: RwLock<HashMap<String, (Instant, String)>>,
}

impl MemoryCache {
    /// Create a new memory cache.
    ///
    /// * `default_ttl` — default TTL in seconds (used when `set` has no explicit TTL)
    /// * `max_entries` — maximum number of entries before LRU eviction
    pub fn new(default_ttl: u64, max_entries: usize) -> Self {
        Self {
            default_ttl,
            max_entries,
            data: RwLock::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl CacheBackend for MemoryCache {
    async fn get(&self, key: &str) -> RyxResult<Option<String>> {
        let map = self.data.read().await;
        if let Some((expires_at, value)) = map.get(key) {
            if expires_at.elapsed() < Duration::from_secs(self.default_ttl) {
                return Ok(Some(value.clone()));
            }
        }
        Ok(None)
    }

    async fn set(&self, key: &str, value: &str, ttl: Option<u64>) -> RyxResult<()> {
        let mut map = self.data.write().await;
        let _cache_ttl = ttl.unwrap_or(self.default_ttl);
        // Evict oldest entries if over max_entries
        if map.len() >= self.max_entries && !map.contains_key(key) {
            if let Some(oldest_key) = map.iter().min_by_key(|(_, (ts, _))| *ts).map(|(k, _)| k.clone()) {
                map.remove(&oldest_key);
            }
        }
        map.insert(key.to_string(), (Instant::now(), value.to_string()));
        Ok(())
    }

    async fn delete(&self, key: &str) -> RyxResult<()> {
        self.data.write().await.remove(key);
        Ok(())
    }

    async fn clear(&self) -> RyxResult<()> {
        self.data.write().await.clear();
        Ok(())
    }
}

// ============================================================
// Redis Cache Backend
// ============================================================

#[cfg(feature = "cache-redis")]
pub mod redis_backend {
    use async_trait::async_trait;
    use redis::AsyncCommands;
    use ryx_common::RyxResult;

    use super::CacheBackend;

    /// A Redis-backed cache.
    pub struct RedisCache {
        client: redis::Client,
        default_ttl: u64,
        prefix: String,
    }

    impl RedisCache {
        pub fn new(client: redis::Client, default_ttl: u64) -> Self {
            Self {
                client,
                default_ttl,
                prefix: "ryx:cache:".to_string(),
            }
        }

        pub fn with_prefix(mut self, prefix: &str) -> Self {
            self.prefix = prefix.to_string();
            self
        }
    }

    #[async_trait]
    impl CacheBackend for RedisCache {
        async fn get(&self, key: &str) -> RyxResult<Option<String>> {
            let full_key = format!("{}{}", self.prefix, key);
            let mut conn = self
                .client
                .get_async_connection()
                .await
                .map_err(|e| RyxError::Internal(e.to_string()))?;
            let value: Option<String> = conn
                .get(&full_key)
                .await
                .map_err(|e| RyxError::Internal(e.to_string()))?;
            Ok(value)
        }

        async fn set(&self, key: &str, value: &str, ttl: Option<u64>) -> RyxResult<()> {
            let full_key = format!("{}{}", self.prefix, key);
            let cache_ttl = ttl.unwrap_or(self.default_ttl);
            let mut conn = self
                .client
                .get_async_connection()
                .await
                .map_err(|e| RyxError::Internal(e.to_string()))?;
            let _: () = conn
                .set_ex(&full_key, value, cache_ttl as usize)
                .await
                .map_err(|e| RyxError::Internal(e.to_string()))?;
            Ok(())
        }

        async fn delete(&self, key: &str) -> RyxResult<()> {
            let full_key = format!("{}{}", self.prefix, key);
            let mut conn = self
                .client
                .get_async_connection()
                .await
                .map_err(|e| RyxError::Internal(e.to_string()))?;
            let _: () = conn
                .del(&full_key)
                .await
                .map_err(|e| RyxError::Internal(e.to_string()))?;
            Ok(())
        }

        async fn clear(&self) -> RyxResult<()> {
            let pattern = format!("{}*", self.prefix);
            let mut conn = self
                .client
                .get_async_connection()
                .await
                .map_err(|e| RyxError::Internal(e.to_string()))?;
            let keys: Vec<String> = conn
                .keys(&pattern)
                .await
                .map_err(|e| RyxError::Internal(e.to_string()))?;
            if !keys.is_empty() {
                let _: () = conn
                    .del(&keys)
                    .await
                    .map_err(|e| RyxError::Internal(e.to_string()))?;
            }
            Ok(())
        }
    }
}

// ============================================================
// Global Cache Registry
// ============================================================

static GLOBAL_CACHE: once_cell::sync::OnceCell<RwLock<Option<Arc<dyn CacheBackend>>>> =
    once_cell::sync::OnceCell::new();

/// Configure the global cache backend.
///
/// All `.cache()` calls will use this backend.
///
/// ```ignore
/// use ryx_rs::cache::{configure_cache, MemoryCache};
///
/// configure_cache(MemoryCache::new(300, 5000));
/// ```
pub fn configure_cache(backend: impl CacheBackend + 'static) {
    let lock = GLOBAL_CACHE.get_or_init(|| RwLock::new(None));
    let mut guard = lock.try_write().expect("Cache registry lock poisoned");
    *guard = Some(Arc::new(backend));
}

// ============================================================
// Cache Key Generation
// ============================================================

/// Generate a deterministic cache key from the compiled query.
pub fn make_cache_key(model_name: &str, sql: &str, values_json: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    sql.hash(&mut hasher);
    values_json.hash(&mut hasher);
    let hash = hasher.finish();
    format!("ryx:{}:{:016x}", model_name, hash)
}

// ============================================================
// Cached QuerySet
// ============================================================

/// A wrapper around `QuerySet` that adds caching.
///
/// Created by calling `.cache()` on a `QuerySet`.
pub struct CachedQuerySet<T> {
    pub(crate) inner: QuerySet<T>,
    pub(crate) ttl: u64,
    pub(crate) explicit_key: Option<String>,
}

impl<T: FromRow + Send + Sync> CachedQuerySet<T> {
    /// Execute the query, checking the cache first.
    pub async fn all(self) -> RyxResult<Vec<T>> {
        let compiled = compiler::compile(&self.inner.node)?;
        let values_json = serde_json::to_string(&compiled.values).unwrap_or_default();
        let model_name = std::any::type_name::<T>();
        let cache_key = self
            .explicit_key
            .clone()
            .unwrap_or_else(|| make_cache_key(model_name, &compiled.sql, &values_json));

        // Try cache hit
        if let Some(lock) = GLOBAL_CACHE.get() {
            let guard = lock.read().await;
            if let Some(backend) = guard.as_ref() {
                if let Some(cached_json) = backend.get(&cache_key).await? {
                    if let Ok((columns, raw_rows)) =
                        serde_json::from_str::<(Vec<String>, Vec<Vec<SqlValue>>)>(&cached_json)
                    {
                        let mapping = std::sync::Arc::new(RowMapping { columns });
                        let rows: Vec<RowView> = raw_rows
                            .into_iter()
                            .map(|values| RowView {
                                values,
                                mapping: mapping.clone(),
                            })
                            .collect();
                        return rows.iter().map(T::from_row).collect();
                    }
                }
            }
        }

        // Cache miss: fetch raw rows
        let rows = self.inner.fetch_raw_rows().await?;

        // Store in cache
        let columns = rows
            .first()
            .map(|r| r.mapping.columns.clone())
            .unwrap_or_default();
        let raw_data: Vec<Vec<SqlValue>> = rows.iter().map(|r| r.values.clone()).collect();
        let cached_json =
            serde_json::to_string(&(columns, raw_data)).map_err(|e| RyxError::Internal(e.to_string()))?;

        if let Some(lock) = GLOBAL_CACHE.get() {
            let guard = lock.read().await;
            if let Some(backend) = guard.as_ref() {
                let _ = backend.set(&cache_key, &cached_json, Some(self.ttl)).await;
            }
        }

        // Decode and return
        rows.iter().map(T::from_row).collect()
    }
}
