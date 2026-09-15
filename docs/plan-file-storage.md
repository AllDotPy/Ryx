# Plan d'implémentation — Champs Fichiers/Images + Storages pluggables

> `FileField` / `ImageField` avec backends de stockage extensibles (local par défaut), en **Python et Rust**.

## Décisions de conception

| Décision | Choix |
|---|---|
| API storage | **Async** — `save/open/delete/exists/url/size/path/get_available_name`, miroir de `AbstractCache` / `RyxBackend` |
| Déclencheur d'écriture | **Auto + explicite** — assignment stage, `Model.save()` commit ; `await ff.save(name, content)` commit immédiat |
| ImageField | **Pillow optionnel** (extra `ryx[images]`) ; fallback magic-bytes sans dépendance |
| Stockage des octets | **Externe** — la DB ne stocke que le chemin/nom (`VARCHAR`). Aucun changement de schéma DB |
| Extensibilité | Trait/ABC public + registry global `configure_storage()` / `get_storage()` |
| Backends non-PG | Aucun impact : le chemin est du texte, compatible MySQL/SQLite |

**Principe directeur** : aucune régression. Tout est additif (nouveaux modules, nouveaux champs, hooks no-op par défaut). Pas de nouveau `SqlValue`, pas de changement DDL, pas de changement backend.

---

## Contrat du trait `Storage` (commun Python/Rust)

| Méthode | Signature | Rôle |
|---|---|---|
| `save` | `(name, content: bytes) -> str` | Écrit les octets, retourne le nom final (collisions résolues) |
| `open` | `(name) -> bytes` | Lit les octets |
| `delete` | `(name) -> None` | Supprime (idempotent) |
| `exists` | `(name) -> bool` | Teste l'existence |
| `url` | `(name) -> str` | URL publique |
| `size` | `(name) -> int` | Taille en octets |
| `path` | `(name) -> str` | Chemin local (peut lever si distant) |
| `get_available_name` | `(name) -> str` | Résout les collisions |
| `listdir` | `(path="") -> list[tuple[str, str]]` | (optionnel) Listing |

Tous les backends doivent implémenter ces méthodes. Un backend custom (S3, GCS, Azure Blob…) implémente le même contrat.

---

## Architecture cible

```
Python                                    Rust
─────────────────────────────             ─────────────────────────────
FileField / ImageField (fields.py)        StoredFile (storage.rs)
   │  stage un FieldFile                     │
   ▼                                         ▼
FieldFile (files.py)                       Storage trait (async_trait)
   │  await save/open/delete/url             │
   ▼                                         ▼
Storage ABC (storage.py)  ◄── même contrat ──►  Storage trait (storage.rs)
   ├─ FileSystemStorage (défaut)              ├─ LocalStorage (défaut)
   ├─ InMemoryStorage (tests)                 ├─ InMemoryStorage (tests)
   └─ custom (utilisateur)                    └─ custom (utilisateur)
   ▲                                         ▲
configure_storage() / get_storage()       configure_storage() / get_storage()
```

---

## Phase 1 — Cœur storage Rust (`ryx-rs/src/storage.rs`)

**Nouveau module**, calqué sur `ryx-rs/src/cache.rs` (le pattern `CacheBackend` + registry global).

### 1.1 Trait `Storage`

```rust
#[async_trait::async_trait]
pub trait Storage: Send + Sync {
    async fn save(&self, name: &str, content: &[u8]) -> RyxResult<String>;
    async fn open(&self, name: &str) -> RyxResult<Vec<u8>>;
    async fn delete(&self, name: &str) -> RyxResult<()>;
    async fn exists(&self, name: &str) -> RyxResult<bool>;
    fn url(&self, name: &str) -> String;
    async fn size(&self, name: &str) -> RyxResult<u64>;
    fn path(&self, name: &str) -> RyxResult<std::path::PathBuf>;  // LocalStorage only
    fn get_available_name(&self, name: &str) -> String;
    async fn listdir(&self, path: &str) -> RyxResult<Vec<(String, String)>>;
}
```

### 1.2 `LocalStorage`

```rust
pub struct LocalStorage {
    pub root: PathBuf,
    pub base_url: String,
    pub file_permissions: Option<u32>,
    pub directory_permissions: Option<u32>,
}
```

- `save` : crée les dossiers parents, écrit via `tokio::fs`, retourne `get_available_name`.
- Résolution des collisions : `photo.jpg` → `photo_a1b2c3.jpg` (suffixe aléatoire).
- Sécurité : refuse les chemins hors de `root` (`../`).

### 1.3 `InMemoryStorage`

`Arc<RwLock<HashMap<String, Vec<u8>>>>` — pour les tests, sans I/O disque.

### 1.4 Registry global

```rust
static GLOBAL_STORAGE: OnceCell<RwLock<Option<Arc<dyn Storage>>>> = OnceCell::new();
pub fn configure_storage(s: impl Storage + 'static);
pub fn get_storage() -> Option<Arc<dyn Storage>>;
pub fn clear_storage();
```

### 1.5 Export (`ryx-rs/src/lib.rs`)

```rust
pub mod storage;
pub use storage::{Storage, LocalStorage, InMemoryStorage, configure_storage, get_storage};
```

---

## Phase 2 — `StoredFile` + intégration Rust

### 2.1 Type `StoredFile`

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredFile(String);

impl StoredFile {
    pub fn new(name: impl Into<String>) -> Self;
    pub fn name(&self) -> &str;
    pub async fn save(storage: &dyn Storage, name: &str, content: &[u8]) -> RyxResult<Self>;
    pub async fn open(&self, storage: &dyn Storage) -> RyxResult<Vec<u8>>;
    pub async fn delete(&self, storage: &dyn Storage) -> RyxResult<()>;
    pub fn url(&self, storage: &dyn Storage) -> String;
}
```

### 2.2 Conversions

- `impl IntoSqlValue for StoredFile` → `SqlValue::Text(self.0)` (`ryx-rs/src/into_sql.rs`).
- `impl FromStr for StoredFile` → décode depuis `Text` via le fallback existant du macro (`ryx-macro/src/lib.rs:386-393`). Aucun changement de macro obligatoire, mais on peut ajouter un bras explicite dans `type_to_sql_reader_expr`.

### 2.3 Macro (`ryx-macro/src/lib.rs`)

- `FieldAttr` : ajouter `upload_to: Option<String>` (+ `max_length`).
- Parser : bras `strip_prefix("upload_to = ")` (ligne ~18-68).
- `rust_type_to_sql` : bras `"StoredFile" => "VARCHAR(255)"` (ligne ~182-206).
- Optionnel : bras de décodage explicite dans `type_to_sql_reader_expr` (~308-400).

### 2.4 Config (`ryx-rs/src/config.rs`)

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StorageConfig {
    pub backend: Option<String>,   // "local" | "memory" | custom
    pub root: Option<String>,
    pub base_url: Option<String>,
}
// Ajouter `#[serde(default)] pub storage: StorageConfig` à RyxConfig + Default
```
Overrides env dans `apply_env_overrides` : `RYX_STORAGE_ROOT`, `RYX_STORAGE_BASE_URL`.

---

## Phase 3 — Cœur storage Python (`ryx/storage.py`)

**Nouveau module**, calqué sur `ryx/cache.py` (`AbstractCache` + `MemoryCache` + `configure_cache`/`get_cache`).

### 3.1 ABC `Storage`

```python
class Storage(ABC):
    @abstractmethod
    async def save(self, name: str, content: bytes) -> str: ...
    @abstractmethod
    async def open(self, name: str) -> bytes: ...
    @abstractmethod
    async def delete(self, name: str) -> None: ...
    @abstractmethod
    async def exists(self, name: str) -> bool: ...
    @abstractmethod
    def url(self, name: str) -> str: ...
    @abstractmethod
    async def size(self, name: str) -> int: ...
    def path(self, name: str) -> str: ...          # FileSystemStorage
    def get_available_name(self, name: str) -> str: ...
    async def listdir(self, path: str = "") -> list[tuple[str, str]]: ...
```

### 3.2 `FileSystemStorage` (défaut)

- `root` (défaut `"media/"`), `base_url` (défaut `"/media/"`).
- I/O via `anyio`/`asyncio.to_thread` (pas de dépendance bloquante) ou `aiofiles` optionnel.
- Collisions : suffixe aléatoire de 7 caractères.
- Sécurité : garde-fou `../`.

### 3.3 `InMemoryStorage` (tests)

Dict en mémoire.

### 3.4 Registry + config

```python
_default_storage: Optional[Storage] = None
def configure_storage(storage: Storage) -> None: ...
def get_storage() -> Storage: ...          # crée un FileSystemStorage par défaut si rien
def default_storage() -> Storage: ...
```

Intégration config (`ryx.toml`) :
```toml
[storage]
backend = "local"
root = "media/"
base_url = "/media/"
```
+ env `RYX_STORAGE_ROOT`, `RYX_STORAGE_BASE_URL` lus dans `_auto_setup()` (`ryx/__init__.py:335-428`) et `ryx/cli/config.py`.

Export dans `ryx/__init__.py` (`__all__`) : `Storage`, `FileSystemStorage`, `InMemoryStorage`, `configure_storage`, `get_storage`.

---

## Phase 4 — `FieldFile` + `FileField` + `ImageField` (Python)

### 4.1 `FieldFile` (`ryx/files.py`, nouveau)

Value object qui enveloppe le nom stocké et référence storage + field + instance.

```python
class FieldFile:
    name: Optional[str]          # chemin stocké (None si vide)
    _committed: bool             # True si déjà écrit dans le storage
    _pending: Optional[bytes]    # contenu stagé non encore commité
    _pending_name: Optional[str]

    async def save(self, name: str, content: bytes) -> None: ...  # commit immédiat
    async def open(self, mode="rb") -> bytes: ...
    async def read(self) -> bytes: ...
    async def delete(self, save: bool = True) -> None: ...
    async def exists(self) -> bool: ...
    def url(self) -> str: ...
    async def size(self) -> int: ...
```

### 4.2 `FileField(Field)` (`ryx/fields.py`)

```python
class FileField(Field):
    SUPPORTED_LOOKUPS = ["exact", "isnull", "in"]
    def __init__(self, *, upload_to="", storage=None, max_length=255, **kw): ...
    def db_type(self) -> str: return f"VARCHAR({self.max_length})"
    def to_python(self, v) -> FieldFile: ...       # wrap le nom en FieldFile
    def to_db(self, v: FieldFile) -> Optional[str]: return v.name
    def __set__(self, obj, value): ...             # str→nom existant ; bytes→stage
    def generate_filename(self, instance, filename) -> str: ...  # applique upload_to
    async def before_save(self, instance, created) -> None: ...  # commit pending
    async def after_delete(self, instance) -> None: ...          # supprime le fichier
```

- `upload_to` : `str` (préfixe) ou `callable(instance, filename) -> str`.
- Descriptor override (précédent : `ForeignKey.contribute_to_class`, `fields.py:963-975`).

### 4.3 `ImageField(FileField)`

```python
class ImageField(FileField):
    def __init__(self, *, width_field=None, height_field=None, **kw): ...
    async def before_save(self, instance, created) -> None:
        await super().before_save(...)
        # si Pillow dispo : ouvre, valide, remplit width/height (et width_field/height_field)
        # sinon : validation magic-bytes (PNG/JPEG/GIF/WEBP) sans dépendance
```

- Pillow via extra : `pip install ryx[images]` (optionnel dans `pyproject.toml`).
- Propriétés `width`, `height` (lazy, lues via Pillow si dispo).

### 4.4 Validators optionnels (`ryx/validators.py`)

- `FileExtensionValidator(allowed_extensions)`
- `FileSizeValidator(max_size)` — en octets
- `ImageValidator` (si Pillow)

---

## Phase 5 — Hooks de cycle de vie (non-régressif)

### 5.1 Hook field (no-op par défaut) — `ryx/fields.py`

```python
class Field:
    async def before_save(self, instance, created: bool) -> None: ...
    async def after_delete(self, instance) -> None: ...
```

### 5.2 Wiring

| Emplacement | Modification |
|---|---|
| `Model.save()` (`models.py:592-596`) | Avant `before_save` du modèle : `for f in fields: await f.before_save(self, created)` |
| `Model.delete()` (`models.py:682-712`) | Après SQL : `for f in fields: await f.after_delete(self)` |
| `bulk_create` (`bulk.py:135-136`) | Commit des pending avant construction des rows |
| `bulk_update` (`bulk.py:322-323`) | Idem |
| `QuerySet.update()` (`queryset.py:821`) | ⚠️ Ne passe pas par `to_db()` — **documenter la limitation** (les FileField ne doivent pas être modifiés via `.update()`) |

Les hooks étant no-op sur `Field` de base, **aucun comportement existant n'est modifié**.

### 5.3 Nettoyage à la suppression

- `Model.delete()` appelle `after_delete` par champ → `FileField` supprime le fichier.
- `QuerySet.delete()` / `bulk_delete` n'ont pas d'instances → **ne nettoient pas** les fichiers (documenté). Option future : fetch des noms avant delete.

---

## Phase 6 — Tests

### Rust
- `ryx-rs/src/storage.rs` (tests unitaires) :
  - `LocalStorage` : save/open/delete/exists/url/size, collisions, refus `../`
  - `InMemoryStorage` : mêmes opérations
  - Registry : `configure_storage`/`get_storage`
  - `StoredFile` : conversions `IntoSqlValue`, `FromStr`
- Test d'intégration SQLite : modèle avec `StoredFile`, insert + relecture.

### Python
- `tests/unit/test_storage.py` : `FileSystemStorage`, `InMemoryStorage` (save/open/delete/exists/url/size, collisions, sécurité)
- `tests/unit/test_files.py` : `FieldFile` (staging, commit, delete, url)
- `tests/unit/test_fields.py` : `FileField.db_type`, `to_db`, `to_python`, `upload_to` str/callable, `ImageField` (avec/sans Pillow)
- `tests/integration/test_file_storage.py` : E2E — `Item.objects.create(avatar=bytes)` → fichier sur disque → `read()` → `delete()` → fichier supprimé
- Non-régression : suite existante (unit + intégration)

---

## Phase 7 — Documentation

- Nouvelle page `docs/doc/advanced/file-storage.mdx` (Python + Rust) :
  - `FileField` / `ImageField`, `upload_to`, `storage`
  - `configure_storage()`, `FileSystemStorage`, backends custom
  - `FieldFile` API (`save`, `open`, `delete`, `url`, `size`)
  - Extra `ryx[images]` (Pillow)
  - Limitation `.update()`
- `docs/doc/reference/field-reference.mdx` : `FileField`, `ImageField`
- `README.md` : ligne « File/Image fields + pluggable storage ✅ »
- `docs/doc/advanced/index.mdx` : lien

---

## Fichiers touchés (récapitulatif)

| Fichier | Changement |
|---|---|
| `ryx-rs/src/storage.rs` | **Nouveau** — trait `Storage`, `LocalStorage`, `InMemoryStorage`, registry, `StoredFile` |
| `ryx-rs/src/lib.rs` | Exports storage |
| `ryx-rs/src/into_sql.rs` | `IntoSqlValue for StoredFile` |
| `ryx-rs/src/config.rs` | `StorageConfig` + env overrides |
| `ryx-macro/src/lib.rs` | `FieldAttr.upload_to` + parser + `rust_type_to_sql` arm |
| `ryx-python/ryx/storage.py` | **Nouveau** — ABC `Storage`, `FileSystemStorage`, `InMemoryStorage`, registry |
| `ryx-python/ryx/files.py` | **Nouveau** — `FieldFile` |
| `ryx-python/ryx/fields.py` | `FileField`, `ImageField`, hooks `before_save`/`after_delete` |
| `ryx-python/ryx/validators.py` | `FileExtensionValidator`, `FileSizeValidator`, `ImageValidator` |
| `ryx-python/ryx/models.py` | Wiring hooks dans `save()`/`delete()` |
| `ryx-python/ryx/bulk.py` | Commit pending dans `bulk_create`/`bulk_update` |
| `ryx-python/ryx/__init__.py` | Exports + config storage |
| `ryx-python/ryx/cli/config.py` | Section `storage` |
| `ryx-python/pyproject.toml` | Extra `images` (Pillow) |
| `docs/doc/advanced/file-storage.mdx` | **Nouveau** |
| `docs/doc/reference/field-reference.mdx` | `FileField`/`ImageField` |
| `README.md` | Ligne feature |

---

## Ordre d'implémentation recommandé

1. **Phase 1** — Cœur storage Rust (trait + Local + InMemory + registry) — isolé, testable
2. **Phase 3** — Cœur storage Python (ABC + FileSystem + InMemory + registry) — isolé, testable
3. **Phase 2** — `StoredFile` + macro + config Rust
4. **Phase 4** — `FieldFile` + `FileField` + `ImageField` Python
5. **Phase 5** — Hooks de cycle de vie (additif, no-op par défaut)
6. **Phase 6** — Tests (unit puis E2E)
7. **Phase 7** — Documentation

---

## Points d'attention (risques)

| Risque | Mitigation |
|---|---|
| I/O bloquant dans `__set__` (sync) | Le staging ne fait aucune I/O ; le commit est async dans `save()` |
| `QuerySet.update()` saute `to_db()` | Documenter : ne pas modifier un FileField via `.update()` |
| Nettoyage fichiers partagés | Documenter la limite ; `after_delete` best-effort |
| `bulk_delete` sans instance | Pas de nettoyage automatique ; documenté |
| Pillow absent | Fallback magic-bytes, pas d'import dur |
| Sécurité chemins (`../`) | Garde-fou dans `FileSystemStorage`/`LocalStorage` |
| Régressions hooks | Hooks no-op sur `Field` de base ; wiring conditionnel |
| Config inconnue | `#[serde(default)]` / `.get()` tolérant |

---

## Estimations

| Phase | Lignes estimées | Risque |
|---|---|---|
| 1. Storage Rust | ~350 | Faible |
| 2. StoredFile + macro + config | ~150 | Moyen (macro) |
| 3. Storage Python | ~300 | Faible |
| 4. FieldFile/FileField/ImageField | ~400 | Moyen |
| 5. Hooks cycle de vie | ~80 | Moyen (non-régression) |
| 6. Tests | ~450 | Moyen |
| 7. Docs | ~250 | Nul |
| **Total** | **~1980** | |
