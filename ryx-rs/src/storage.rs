//
// ###
// Ryx — Storage backends for FileField / ImageField
// ###
//
// A pluggable storage abstraction. The default backend is the local
// filesystem; users can implement `Storage` for S3, GCS, Azure Blob, etc.
// and register it globally with `configure_storage()`.
//
// Mirrors the `cache.rs` pattern (trait + built-in impls + global registry).
// ###

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use ryx_common::{RyxError, RyxResult};
use tokio::sync::RwLock;

// ============================================================
// Storage trait
// ============================================================

/// A pluggable file storage backend.
///
/// Implement this trait to store files anywhere (local disk, S3, GCS,
/// Azure Blob, …). The default backend is [`LocalStorage`].
///
/// ```ignore
/// use ryx_rs::storage::{Storage, LocalStorage, configure_storage};
///
/// configure_storage(LocalStorage::new("media/").with_base_url("/media/"));
/// ```
#[async_trait]
pub trait Storage: Send + Sync {
    /// Write `content` under `name` and return the final stored name.
    ///
    /// Implementations must resolve collisions (e.g. `a.jpg` → `a_x1y2.jpg`)
    /// so an existing file is never silently overwritten.
    async fn save(&self, name: &str, content: &[u8]) -> RyxResult<String>;

    /// Read the bytes stored under `name`.
    async fn open(&self, name: &str) -> RyxResult<Vec<u8>>;

    /// Delete the file stored under `name`. Must be idempotent.
    async fn delete(&self, name: &str) -> RyxResult<()>;

    /// Return `true` if a file exists under `name`.
    async fn exists(&self, name: &str) -> RyxResult<bool>;

    /// Return a public URL for `name`.
    fn url(&self, name: &str) -> String;

    /// Return the size of the stored file in bytes.
    async fn size(&self, name: &str) -> RyxResult<u64>;

    /// Return the local filesystem path for `name`, if the backend exposes one.
    ///
    /// Remote backends (S3, …) should return an error.
    fn path(&self, name: &str) -> RyxResult<PathBuf> {
        Err(RyxError::Internal(format!(
            "Storage backend does not expose local paths: {name}"
        )))
    }

    /// Resolve a collision-free name for `name`.
    ///
    /// Default: if `name` is free, return it unchanged; otherwise append a
    /// random suffix before the extension (`a.jpg` → `a_a1b2c3d.jpg`).
    async fn get_available_name(&self, name: &str) -> String {
        if !self.exists(name).await.unwrap_or(false) {
            return name.to_string();
        }
        let (stem, ext) = split_name(name);
        for _ in 0..100 {
            let candidate = format!("{stem}_{}{ext}", random_suffix(7));
            if !self.exists(&candidate).await.unwrap_or(false) {
                return candidate;
            }
        }
        // Extremely unlikely fallback.
        format!("{stem}_{}{ext}", random_suffix(16))
    }

    /// List entries under `path` as `(name, kind)` where kind is
    /// `"file"` or `"dir"`. Default: unsupported.
    async fn listdir(&self, _path: &str) -> RyxResult<Vec<(String, String)>> {
        Err(RyxError::Internal(
            "Storage backend does not support listdir".into(),
        ))
    }
}

// ============================================================
// Helpers
// ============================================================

/// Split a name into `(stem, extension)` where extension includes the dot.
fn split_name(name: &str) -> (String, String) {
    match name.rfind('.') {
        Some(idx) if idx > 0 => (name[..idx].to_string(), name[idx..].to_string()),
        _ => (name.to_string(), String::new()),
    }
}

/// Generate a short pseudo-random lowercase hex suffix.
fn random_suffix(len: usize) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let c = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut x = nanos ^ c.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    let hex = format!("{x:016x}");
    hex[..len.min(hex.len())].to_string()
}

/// Reject absolute paths and `..` traversal.
fn validate_relative(name: &str) -> RyxResult<()> {
    let p = Path::new(name);
    if p.is_absolute() {
        return Err(RyxError::Internal(format!(
            "Storage path must be relative: {name}"
        )));
    }
    for comp in p.components() {
        if matches!(comp, Component::ParentDir) {
            return Err(RyxError::Internal(format!(
                "Storage path must not contain '..': {name}"
            )));
        }
    }
    Ok(())
}

// ============================================================
// Local filesystem storage (default)
// ============================================================

/// Filesystem-backed storage rooted at `root`.
///
/// ```ignore
/// let s = LocalStorage::new("media/").with_base_url("/media/");
/// let name = s.save("avatars/a.png", &bytes).await?;
/// ```
pub struct LocalStorage {
    /// Root directory on disk (created on demand).
    pub root: PathBuf,
    /// Public URL prefix (e.g. `/media/` or `https://cdn.example.com/`).
    pub base_url: String,
}

impl LocalStorage {
    /// Create a local storage rooted at `root`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            base_url: "/media/".to_string(),
        }
    }

    /// Set the public URL prefix.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    /// Resolve a validated absolute path under `root`.
    fn safe_path(&self, name: &str) -> RyxResult<PathBuf> {
        validate_relative(name)?;
        Ok(self.root.join(name))
    }
}

#[async_trait]
impl Storage for LocalStorage {
    async fn save(&self, name: &str, content: &[u8]) -> RyxResult<String> {
        let available = self.get_available_name(name).await;
        let full = self.safe_path(&available)?;
        if let Some(parent) = full.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| RyxError::Internal(format!("Failed to create dir: {e}")))?;
        }
        tokio::fs::write(&full, content)
            .await
            .map_err(|e| RyxError::Internal(format!("Failed to write file: {e}")))?;
        Ok(available)
    }

    async fn open(&self, name: &str) -> RyxResult<Vec<u8>> {
        let full = self.safe_path(name)?;
        tokio::fs::read(&full)
            .await
            .map_err(|e| RyxError::Internal(format!("Failed to read file '{name}': {e}")))
    }

    async fn delete(&self, name: &str) -> RyxResult<()> {
        let full = self.safe_path(name)?;
        match tokio::fs::remove_file(&full).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(RyxError::Internal(format!("Failed to delete file: {e}"))),
        }
    }

    async fn exists(&self, name: &str) -> RyxResult<bool> {
        let full = self.safe_path(name)?;
        Ok(tokio::fs::metadata(&full).await.is_ok())
    }

    fn url(&self, name: &str) -> String {
        let base = self.base_url.trim_end_matches('/');
        let name = name.trim_start_matches('/');
        format!("{base}/{name}")
    }

    async fn size(&self, name: &str) -> RyxResult<u64> {
        let full = self.safe_path(name)?;
        let meta = tokio::fs::metadata(&full)
            .await
            .map_err(|e| RyxError::Internal(format!("Failed to stat file: {e}")))?;
        Ok(meta.len())
    }

    fn path(&self, name: &str) -> RyxResult<PathBuf> {
        self.safe_path(name)
    }

    async fn listdir(&self, path: &str) -> RyxResult<Vec<(String, String)>> {
        let dir = if path.is_empty() {
            self.root.clone()
        } else {
            self.safe_path(path)?
        };
        let mut out = Vec::new();
        let mut rd = tokio::fs::read_dir(&dir)
            .await
            .map_err(|e| RyxError::Internal(format!("Failed to list dir: {e}")))?;
        while let Some(entry) = rd
            .next_entry()
            .await
            .map_err(|e| RyxError::Internal(format!("Failed to read dir entry: {e}")))?
        {
            let name = entry.file_name().to_string_lossy().to_string();
            let kind = if entry.path().is_dir() { "dir" } else { "file" };
            out.push((name, kind.to_string()));
        }
        Ok(out)
    }
}

// ============================================================
// In-memory storage (tests / ephemeral)
// ============================================================

/// An in-memory storage backend. Useful for tests.
pub struct InMemoryStorage {
    data: RwLock<HashMap<String, Vec<u8>>>,
    base_url: String,
}

impl InMemoryStorage {
    pub fn new() -> Self {
        Self {
            data: RwLock::new(HashMap::new()),
            base_url: "/media/".to_string(),
        }
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }
}

impl Default for InMemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Storage for InMemoryStorage {
    async fn save(&self, name: &str, content: &[u8]) -> RyxResult<String> {
        validate_relative(name)?;
        let available = self.get_available_name(name).await;
        self.data
            .write()
            .await
            .insert(available.clone(), content.to_vec());
        Ok(available)
    }

    async fn open(&self, name: &str) -> RyxResult<Vec<u8>> {
        self.data
            .read()
            .await
            .get(name)
            .cloned()
            .ok_or_else(|| RyxError::Internal(format!("File not found: {name}")))
    }

    async fn delete(&self, name: &str) -> RyxResult<()> {
        self.data.write().await.remove(name);
        Ok(())
    }

    async fn exists(&self, name: &str) -> RyxResult<bool> {
        Ok(self.data.read().await.contains_key(name))
    }

    fn url(&self, name: &str) -> String {
        let base = self.base_url.trim_end_matches('/');
        let name = name.trim_start_matches('/');
        format!("{base}/{name}")
    }

    async fn size(&self, name: &str) -> RyxResult<u64> {
        self.data
            .read()
            .await
            .get(name)
            .map(|v| v.len() as u64)
            .ok_or_else(|| RyxError::Internal(format!("File not found: {name}")))
    }

    async fn listdir(&self, path: &str) -> RyxResult<Vec<(String, String)>> {
        let prefix = if path.is_empty() {
            String::new()
        } else {
            format!("{}/", path.trim_end_matches('/'))
        };
        let mut out = Vec::new();
        for key in self.data.read().await.keys() {
            if let Some(rest) = key.strip_prefix(&prefix) {
                if !rest.contains('/') {
                    out.push((rest.to_string(), "file".to_string()));
                }
            }
        }
        Ok(out)
    }
}

// ============================================================
// Global storage registry
// ============================================================

static GLOBAL_STORAGE: once_cell::sync::OnceCell<RwLock<Option<Arc<dyn Storage>>>> =
    once_cell::sync::OnceCell::new();

/// Configure the global default storage backend.
///
/// ```ignore
/// use ryx_rs::storage::{configure_storage, LocalStorage};
/// configure_storage(LocalStorage::new("media/"));
/// ```
pub fn configure_storage(storage: impl Storage + 'static) {
    let lock = GLOBAL_STORAGE.get_or_init(|| RwLock::new(None));
    let mut guard = lock.try_write().expect("Storage registry lock poisoned");
    *guard = Some(Arc::new(storage));
}

/// Return the configured global storage backend, if any.
pub fn get_storage() -> Option<Arc<dyn Storage>> {
    let lock = GLOBAL_STORAGE.get()?;
    let guard = lock.try_read().ok()?;
    guard.clone()
}

/// Clear the global storage backend.
pub fn clear_storage() {
    if let Some(lock) = GLOBAL_STORAGE.get() {
        if let Ok(mut guard) = lock.try_write() {
            *guard = None;
        }
    }
}

// ============================================================
// StoredFile — a value stored via a Storage backend
// ============================================================

/// A reference to a file stored via a [`Storage`] backend.
///
/// Persisted as its stored name/path (a `TEXT` column). Holds no open
/// handle — call [`StoredFile::open`] with a storage backend to read it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StoredFile(String);

impl StoredFile {
    /// Wrap an existing stored name.
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    /// The stored name/path.
    pub fn name(&self) -> &str {
        &self.0
    }

    /// Consume and return the stored name.
    pub fn into_inner(self) -> String {
        self.0
    }

    /// Whether this value is empty (no file).
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Save `content` via `storage` and return the resulting [`StoredFile`].
    pub async fn save(
        storage: &dyn Storage,
        name: &str,
        content: &[u8],
    ) -> RyxResult<Self> {
        Ok(Self(storage.save(name, content).await?))
    }

    /// Read the bytes via `storage`.
    pub async fn open(&self, storage: &dyn Storage) -> RyxResult<Vec<u8>> {
        storage.open(&self.0).await
    }

    /// Delete the file via `storage` (idempotent).
    pub async fn delete(&self, storage: &dyn Storage) -> RyxResult<()> {
        storage.delete(&self.0).await
    }

    /// Return the public URL via `storage`.
    pub fn url(&self, storage: &dyn Storage) -> String {
        storage.url(&self.0)
    }

    /// Return the file size via `storage`.
    pub async fn size(&self, storage: &dyn Storage) -> RyxResult<u64> {
        storage.size(&self.0).await
    }

    /// Return `true` if the file exists via `storage`.
    pub async fn exists(&self, storage: &dyn Storage) -> RyxResult<bool> {
        storage.exists(&self.0).await
    }
}

impl std::fmt::Display for StoredFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for StoredFile {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for StoredFile {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl std::str::FromStr for StoredFile {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(s.to_string()))
    }
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("ryx_storage_{tag}_{}", random_suffix(10)))
    }

    #[tokio::test]
    async fn test_memory_save_open_exists_delete() {
        let s = InMemoryStorage::new();
        let name = s.save("a/b.txt", b"hello").await.unwrap();
        assert_eq!(name, "a/b.txt");
        assert!(s.exists("a/b.txt").await.unwrap());
        assert_eq!(s.open("a/b.txt").await.unwrap(), b"hello");
        assert_eq!(s.size("a/b.txt").await.unwrap(), 5);
        s.delete("a/b.txt").await.unwrap();
        assert!(!s.exists("a/b.txt").await.unwrap());
        // delete is idempotent
        s.delete("a/b.txt").await.unwrap();
    }

    #[tokio::test]
    async fn test_memory_collision_resolution() {
        let s = InMemoryStorage::new();
        let a = s.save("f.txt", b"1").await.unwrap();
        let b = s.save("f.txt", b"2").await.unwrap();
        assert_eq!(a, "f.txt");
        assert_ne!(a, b);
        assert!(b.starts_with("f_"));
        assert!(b.ends_with(".txt"));
        assert_eq!(s.open(&a).await.unwrap(), b"1");
        assert_eq!(s.open(&b).await.unwrap(), b"2");
    }

    #[tokio::test]
    async fn test_memory_url() {
        let s = InMemoryStorage::new().with_base_url("https://cdn.example.com/media");
        assert_eq!(s.url("x/y.png"), "https://cdn.example.com/media/x/y.png");
    }

    #[tokio::test]
    async fn test_memory_rejects_traversal() {
        let s = InMemoryStorage::new();
        assert!(s.save("../evil.txt", b"x").await.is_err());
        assert!(s.save("/abs.txt", b"x").await.is_err());
    }

    #[tokio::test]
    async fn test_local_save_open_delete() {
        let root = temp_root("local");
        let s = LocalStorage::new(&root).with_base_url("/media/");
        let name = s.save("avatars/a.png", b"PNGDATA").await.unwrap();
        assert_eq!(name, "avatars/a.png");
        assert!(s.exists("avatars/a.png").await.unwrap());
        assert_eq!(s.open("avatars/a.png").await.unwrap(), b"PNGDATA");
        assert_eq!(s.size("avatars/a.png").await.unwrap(), 7);
        assert!(root.join("avatars/a.png").exists());
        assert_eq!(s.url("avatars/a.png"), "/media/avatars/a.png");
        s.delete("avatars/a.png").await.unwrap();
        assert!(!s.exists("avatars/a.png").await.unwrap());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn test_local_collision_resolution() {
        let root = temp_root("collision");
        let s = LocalStorage::new(&root);
        let a = s.save("f.txt", b"1").await.unwrap();
        let b = s.save("f.txt", b"2").await.unwrap();
        assert_eq!(a, "f.txt");
        assert_ne!(a, b);
        assert!(b.starts_with("f_"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn test_local_rejects_traversal() {
        let root = temp_root("trav");
        let s = LocalStorage::new(&root);
        assert!(s.save("../evil.txt", b"x").await.is_err());
        assert!(s.open("../../etc/passwd").await.is_err());
    }

    #[tokio::test]
    async fn test_stored_file_roundtrip() {
        let s = InMemoryStorage::new();
        let sf = StoredFile::save(&s, "docs/report.pdf", b"PDF").await.unwrap();
        assert_eq!(sf.name(), "docs/report.pdf");
        assert_eq!(sf.open(&s).await.unwrap(), b"PDF");
        assert!(sf.exists(&s).await.unwrap());
        assert_eq!(sf.size(&s).await.unwrap(), 3);
        assert_eq!(sf.url(&s), "/media/docs/report.pdf");
        sf.delete(&s).await.unwrap();
        assert!(!sf.exists(&s).await.unwrap());
    }

    #[tokio::test]
    async fn test_stored_file_from_str() {
        let sf: StoredFile = "abc.txt".parse().unwrap();
        assert_eq!(sf.name(), "abc.txt");
        assert_eq!(StoredFile::from("x"), StoredFile::new("x"));
    }

    #[test]
    fn test_registry_configure_and_get() {
        clear_storage();
        assert!(get_storage().is_none());
        configure_storage(InMemoryStorage::new());
        assert!(get_storage().is_some());
        clear_storage();
        assert!(get_storage().is_none());
    }
}
