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
  <a href="https://discord.gg/umDhd5HWgS"><img src="https://img.shields.io/discord/1452761060678303909?style=flat-square&logo=discord" alt="Discord" /></a>
</p>

<p align="center">
  <a href="https://github.com/AllDotPy/Ryx/stargazers"><img src="https://img.shields.io/github/stars/AllDotPy/Ryx?style=social" alt="GitHub stars" /></a>
</p>

---

## Dual-Language ORM

```python
import ryx
from ryx import Model, CharField, Q

class Post(Model):
    title  = CharField(max_length=200)
    views  = IntField(default=0)
    active = BooleanField(default=True)

await ryx.setup("postgres://user:pass@localhost/mydb")
posts = await Post.objects.filter(Q(active=True) | Q(views__gte=1000))
```

```rust
use ryx_rs::model;

#[model]
struct Post {
    #[field(pk)] id: i64,
    title: String,
    views: i64,
    active: bool,
}

let posts = Post::objects()
    .filter(Q::or(Q::new("active", true), Q::new("views__gte", 1000)))
    .all().await?;
```

## Quick Install

```bash
pip install ryx                     # Python
cargo add ryx-rs ryx-macro            # Rust
```

## Documentation

Full docs, guides, API reference: **[ryx.alldotpy.com](https://ryx.alldotpy.com)**

- [Python quick start](https://ryx.alldotpy.com/getting-started/quick-start)
- [Rust quick start](https://ryx.alldotpy.com/getting-started/installation)

## Comparison

| | Diesel | SeaORM | **Ryx (Rust)** |
|---|---|---|---|
| **API style** | Schema-first | Verbose builders | **Django-like** |
| **Q objects (OR/AND/NOT)** | ❌ | ❌ | ✅ |
| **Lookups** | Basic | Basic | **30+** |
| **select_related** | ❌ | ✅ (Eager) | ✅ |
| **Migrations** | Diesel CLI | sea-orm-cli | **Built-in** |
| **PostgreSQL schemas** | ❌ | ❌ | ✅ |
| **Backends** | PG · MySQL · SQLite | PG · MySQL · SQLite | **PG · MySQL · SQLite** |

## Architecture

<p align="center">
   <img src="https://github.com/AllDotPy/Ryx/blob/master/ryx_architecture.svg?raw=true" alt="Ryx Architecture" width="100%" />
</p>

```
          Python (ryx-python)        Rust (ryx-rs)
                │                         │
          PyO3 bridge ────────╗       no pyo3
                │             ║           │
          ┌─────┴─────────────║───────────┴──────┐
          │      ryx-core     ║     ryx-common   │
          └─────┬─────────────║───────────┬──────┘
                │             ║           │
          ┌─────┴─────────────║───────────┴──────┐
          │         ryx-query (SQL compiler)     │
          └───────────────────┬──────────────────┘
                              │
          ┌───────────────────┴──────────────────┐
          │          ryx-backend (sqlx)          │
          │    Postgres · MySQL · SQLite         │
          └──────────────────────────────────────┘
```

## Performance

1 000 rows on SQLite (lower is better):

| Operation | Ryx ORM | SQLAlchemy ORM | SQLAlchemy Core |
|---|---|---|---|
| **bulk_create** | 0.0074 s | 0.1696 s | 0.0022 s |
| **bulk_update** | 0.0023 s | 0.0018 s | 0.0010 s |
| **bulk_delete** | 0.0005 s | 0.0012 s | 0.0009 s |
| **filter + order + limit** | 0.0009 s | 0.0019 s | 0.0008 s |
| **aggregate** | 0.0002 s | 0.0015 s | 0.0005 s |

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md)

## License

Python code: MIT · Rust code: MIT OR Apache-2.0
