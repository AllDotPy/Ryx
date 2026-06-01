<p align="center">
  <img src="https://github.com/AllDotPy/Ryx/blob/master/logo.svg?raw=true" alt="Ryx ORM" width="80" height="80" />
</p>

<h1 align="center">Ryx ORM</h1>

<p align="center">
  <strong>Django-style ORM — Python and Rust. Powered by Rust.</strong>
</p>

<p align="center">
  <a href="https://pypi.org/project/ryx/"><img src="https://img.shields.io/badge/python-3.10%2B-blue?style=for-the-badge&logo=python&logoColor=white" alt="Python 3.10+" /></a>
  <a href="https://crates.io/crates/ryx-rs"><img src="https://img.shields.io/crates/v/ryx-rs?style=for-the-badge&logo=rust&label=ryx-rs" alt="ryx-rs crate" /></a>
  <a href="https://pypi.org/project/ryx/"><img src="https://img.shields.io/pepy/dt/ryx?style=for-the-badge&logo=pypi&logoColor=white&label=python%20downloads" alt="PyPI Downloads" /></a>
  <a href="https://github.com/AllDotPy/Ryx/releases"><img src="https://img.shields.io/pypi/v/ryx?style=for-the-badge" alt="Version" /></a>
  <a href="https://github.com/AllDotPy/Ryx/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-MIT-green?style=for-the-badge" alt="License" /></a>
  <a href="https://github.com/rust-lang/rust"><img src="https://img.shields.io/badge/rust-1.93%2B-orange?style=for-the-badge&logo=rust" alt="Rust 1.83+" /></a>
  <a href="https://discord.gg/umDhd5HWgS">
        <img src="https://img.shields.io/discord/1452761060678303909?style=flat-square&logo=discord" alt="Discord" />
    </a>
</p>

<p align="center">
  <a href="https://github.com/AllDotPy/Ryx/stargazers"><img src="https://img.shields.io/github/stars/AllDotPy/Ryx?style=social" alt="GitHub stars" /></a>
</p>

<p align="center">
    <a href="#-dual-language-orm-choose-your-runtime">Overview</a> •
    <a href="#-python-django-api-accelerated-by-rust">Python</a> •
    <a href="#-rust-same-api-zero-dependencies-on-python">Rust</a> •
    <a href="#-features">Features</a> •
    <a href="#-architecture">Architecture</a> •
    <a href="https://ryx.alldotpy.com">Docs</a> •
    <a href="https://discord.gg/umDhd5HWgS">Discord</a>
</p>

---

## 🌐 Dual-Language ORM — Choose Your Runtime

Ryx delivers the same expressive Django-style ORM API in **both Python and Rust**, backed by a shared ultra-fast query compiler written in Rust. Whether you need async Python with zero GIL blocking, or pure Rust with zero Python dependencies — Ryx has you covered.

```python
# Python — async, GIL-free
posts = await (Post.objects
    .filter(Q(active=True) | Q(views__gte=1000))
    .order_by("-views")
)
```

```rust
// Rust — native, no Python runtime
let posts = Post::objects()
    .filter(Q::or(
        Q::new("active", true),
        Q::new("views__gte", 1000),
    ))
    .order_by("-views")
    .all().await?;
```

---

## 🐍 Python — Django API, Accelerated by Rust

Install: `pip install ryx`

```python
import ryx
from ryx import Model, CharField, IntField, BooleanField, DateTimeField, Q, Count, Sum

class Post(Model):
    title = CharField(max_length=200)
    slug = CharField(max_length=210, unique=True)
    views = IntField(default=0)
    active = BooleanField(default=True)
    created = DateTimeField(auto_now_add=True)
    class Meta:
        ordering = ["-created"]

await ryx.setup("postgres://user:pass@localhost/mydb")

# Queries
posts = await Post.objects.filter(Q(active=True) | Q(views__gte=1000),
    ).exclude(title__startswith="Draft").order_by("-views").limit(20)

# Aggregation
stats = await Post.objects.aggregate(total=Count("id"), avg_views=Avg("views"))

# Transactions
async with ryx.transaction():
    post = await Post.objects.create(title="Atomic post", slug="atomic")
```

| | Django ORM | SQLAlchemy | **Ryx (Python)** |
|---|---|---|---|
| **API** | Ergonomic | Verbose | **Django-identical** |
| **Runtime** | Sync Python | Async Python | **Async Rust** |
| **GIL blocking** | Yes | Yes | **Zero** |
| **Backends** | All | All | **PG · MySQL · SQLite** |
| **Migrations** | Built-in | Alembic | **Built-in** |

---

## 🦀 Rust — Same API, Zero Dependencies on Python

Add to `Cargo.toml`:

```toml
[dependencies]
ryx-rs = "0.1"
ryx-macro = "0.1"

# Or from git:
# ryx-rs = { git = "https://github.com/AllDotPy/Ryx" }
# ryx-macro = { git = "https://github.com/AllDotPy/Ryx" }
```

Define a model:

```rust
use ryx_rs::{Model, FromRow};
use ryx_macro::{Model, FromRow};

#[derive(Model, FromRow)]
#[table(name = "posts")]
struct Post {
    #[field(pk)]
    id: i64,
    title: String,
    slug: String,
    views: i64,
    active: bool,
    created: chrono::NaiveDateTime,
}
```

Query:

```rust
use ryx_rs::{Q, transaction};

// Filter with lookups and Q-objects
let posts: Vec<Post> = Post::objects()
    .filter(Q::or(
        Q::new("active", true),
        Q::new("views__gte", 1000),
    ))
    .filter("title__startswith", "Draft")
    .order_by("-views")
    .limit(20)
    .all().await?;

// Insert
let post = Post::objects().create()
    .set("title", "Hello")
    .set("slug", "hello")
    .save().await?;

// Count
let total = Post::objects().filter("active", true).count().await?;
let exists = Post::objects().filter("slug__startswith", "test").exists().await?;

// Transactions
transaction(|tx| async move {
    Post::objects().filter("author", "bob").delete().await?;
    tx.commit().await
}).await?;
```

| | Diesel | SeaORM | **Ryx (Rust)** |
|---|---|---|---|
| **API** | Schema-first, macros | Verbose builders | **Django-like, concise** |
| **Learning curve** | Steep | Moderate | **Low (Django devs)** |
| **Async** | sync + async | Async | **Async (tokio)** |
| **Q objects** | ❌ | ❌ | **✅ OR / AND / NOT** |
| **Lookups** | Basic | Basic | **30+ lookups & transforms** |
| **Backends** | PG · MySQL · SQLite | PG · MySQL · SQLite | **PG · MySQL · SQLite** |

---

## ⚡ Features

| Feature | `ryx-python` | `ryx-rs` |
|---|---|---|
| **Django-style `.filter()`, `.exclude()`** | ✅ | ✅ |
| **Field lookups** (`__gte`, `__contains`, `__in`, …) | ✅ (30+) | ✅ (30+, depuis `ryx-query`) |
| **Chained transforms** (`created_at__year__gte`) | ✅ | ✅ |
| **Q objects** (OR / AND / NOT) | ✅ | ✅ |
| **Aggregations** (`Count`, `Sum`, `Avg`, …) | ✅ | 🚧 |
| **`select_related` / `prefetch_related`** | ✅ | 🚧 |
| **Transactions** (savepoints, nested) | ✅ | ✅ (closure-based) |
| **Migrations** | ✅ (autogenerated) | 🚧 |
| **Model definition** | Declarative fields | `#[derive(Model)]` macro |
| **Zero-copy row decoding** | ✅ | ✅ |
| **Bulk operations** | ✅ | via `QuerySet` |
| **Custom lookups** | ✅ | ✅ (via registry) |
| **Raw SQL** | ✅ | ✅ (via `ryx_backend::pool`) |

---

## 🏗 Architecture

<p align="center">
   <img src="https://github.com/AllDotPy/Ryx/blob/master/ryx_architecture.svg?raw=true" alt="Ryx Architecture" width="100%" />
</p>

Your queries are compiled to SQL in Rust, executed by sqlx, and decoded back — all without blocking the Python event loop (or requiring it at all on the Rust side).

```
          Python (ryx-python)           Rust (ryx-rs)
                │                            │
          PyO3 bridge ═══════════╗       no pyo3
                │               ║           │
          ┌─────┴───────────────║───────────┴──────┐
          │      ryx-core       ║     ryx-common    │
          │    (shared types)   ║  (errors, config)  │
          └─────┬───────────────║───────────┬──────┘
                │               ║           │
          ┌─────┴───────────────║───────────┴──────┐
          │         ryx-query (SQL compiler)        │
          │   ~248ns per simple lookup compile      │
          └─────────────────────┬──────────────────┘
                                │
          ┌─────────────────────┴──────────────────┐
          │          ryx-backend (sqlx)              │
          │    Postgres · MySQL · SQLite             │
          │    Enum Dispatch — no vtables            │
          └────────────────────────────────────────┘
```

### Key innovation: Zero-Allocation Row View

Instead of creating a dictionary per row, Ryx uses a shared column mapping + flat value vector. This drastically reduces heap allocations and GC pressure during large fetches — in both Python and Rust.

---

## 📊 Performance

Benchmark of 1 000 rows on SQLite (lower is better):

| Operation | Ryx ORM | SQLAlchemy ORM | SQLAlchemy Core | Ryx raw |
|-----------|--------:|---------------:|----------------:|--------:|
| **bulk_create** | 0.0074 s | 0.1696 s | 0.0022 s | 0.0011 s |
| **bulk_update** | 0.0023 s | 0.0018 s | 0.0010 s | 0.0005 s |
| **bulk_delete** | 0.0005 s | 0.0012 s | 0.0009 s | 0.0004 s |
| **filter + order + limit** | 0.0009 s | 0.0019 s | 0.0008 s | 0.0004 s |
| **aggregate** | 0.0002 s | 0.0015 s | 0.0005 s | 0.0001 s |

Ryx ORM is **16× faster** than SQLAlchemy ORM on bulk inserts and **2× faster** on deletes — while keeping the same Django-style API. The raw SQL layer gives you near-C speed when you need it.

**Query compiler**: Simple lookups compile in **~248ns**, complex trees in **~1µs**.

---

## 📚 Documentation

Full documentation with guides, API reference, and examples: **[ryx.alldotpy.com](https://ryx.alldotpy.com)**

- [Python quick start](https://ryx.alldotpy.com/python/quickstart)
- [Rust quick start](https://ryx.alldotpy.com/rust/quickstart)
- [API reference (Python)](https://ryx.alldotpy.com/python/api)
- [API reference (Rust)](https://ryx.alldotpy.com/rust/api/ryx_rs)

---

## 🤝 Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup, architecture details, and contribution guidelines.

## 📄 License

Python code: MIT · Rust code: MIT OR Apache-2.0
