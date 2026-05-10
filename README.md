<p align="center">
  <img src="https://github.com/AllDotPy/Ryx/blob/master/logo.svg?raw=true" alt="Ryx ORM" width="80" height="80" />
</p>

<h1 align="center">Ryx ORM</h1>

<p align="center">
  <strong>Django-style Python ORM. Powered by Rust.</strong>
</p>

<p align="center">
  <a href="https://pypi.org/project/ryx/"><img src="https://img.shields.io/badge/python-3.10%2B-blue?style=for-the-badge&logo=python&logoColor=white" alt="Python 3.10+" /></a>
  <a href="https://pypi.org/project/ryx/"><img src="https://img.shields.io/pepy/dt/ryx?style=for-the-badge&logo=pypi&logoColor=white&label=downloads" alt="PyPI Downloads" /></a>
  <!-- <a href="https://pepy.tech/projects/ryx"><img src="https://static.pepy.tech/badge/ryx?style=for-the-badge" alt="Total Downloads" /></a> -->
  <a href="https://github.com/AllDotPy/Ryx/releases"><img src="https://img.shields.io/pypi/v/ryx?style=for-the-badge" alt="Version" /></a>
  <a href="https://github.com/AllDotPy/Ryx/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-MIT-green?style=for-the-badge" alt="License" /></a>
  <a href="https://github.com/rust-lang/rust"><img src="https://img.shields.io/badge/rust-1.93%2B-orange?style=for-the-badge&logo=rust" alt="Rust 1.83+" /></a>
  <!-- Discord -->
  <a href="https://discord.gg/umDhd5HWgS">
        <img src="https://img.shields.io/discord/1452761060678303909?style=flat-square&logo=discord" alt="Discord" />
    </a>
</p>

<p align="center">
  <a href="https://github.com/AllDotPy/Ryx/stargazers"><img src="https://img.shields.io/github/stars/AllDotPy/Ryx?style=social" alt="GitHub stars" /></a>
</p>

<p align="center">
    <a href="#-quick-start">Quick Start</a> •
    <a href="#-features">Features</a> •
    <a href="#-showcase">Showcase</a> •
    <a href="https://ryx.alldotpy.com">Docs</a> •
    <a href="https://discord.gg/umDhd5HWgS">Discord</a>
</p>

---

Ryx gives you the query API you love — `.filter()`, `Q` objects, aggregations, relationships — with the raw performance of a compiled Rust core. Async-native. Zero event-loop blocking.

```python
import ryx
from ryx import (
    Model, CharField, IntField, BooleanField, 
    DateTimeField, Q, Count, Sum
)

class Post(Model):
    title = CharField(max_length=200)
    slug = CharField(max_length=210, unique=True)
    views = IntField(default=0)
    active = BooleanField(default=True)
    created = DateTimeField(auto_now_add=True)

    class Meta:
        ordering = ["-created"]

# Setup once
await ryx.setup("postgres://user:pass@localhost/mydb")

# Query like Django, run like Rust
posts = await (
    Post.objects
        .filter(Q(active=True) | Q(views__gte=1000))
        .exclude(title__startswith="Draft")
        .order_by("-views")
        .limit(20)
)

# Aggregations
stats = await Post.objects.aggregate(
    total=Count("id"), avg_views=Avg("views"), top=Max("views"),
)

# Transactions with savepoints
async with ryx.transaction():
    post = await Post.objects.create(title="Atomic post", slug="atomic")
    await post.save()
```

## Why Ryx

| | Django ORM | SQLAlchemy | **Ryx** |
|---|---|---|---|
| **API** | Ergonomic | Verbose | **Ergonomic** |
| **Runtime** | Sync Python | Async Python | **Async Rust** |
| **GIL blocking** | Yes | Yes | **Zero** |
| **Backends** | All | All | **PG · MySQL · SQLite** |
| **Migrations** | Built-in | Alembic | **Built-in** |

## Architecture
 
<p align="center">
   <img src="https://github.com/AllDotPy/Ryx/blob/master/ryx_architecture.svg?raw=true" alt="Ryx Architecture" width="100%" />
</p>
 
Your Python queries are compiled to SQL in Rust, executed by sqlx, and decoded back — all without blocking the Python event loop.

To achieve near-native performance, Ryx uses a **multi-crate workspace architecture**:
- `ryx-query`: A standalone, ultra-fast SQL compiler.
- `ryx-backend`: High-performance database drivers using **Enum Dispatch** (no vtables) to eliminate runtime overhead.
- `ryx-core`: Shared base types and the core ORM engine.
- `ryx-python`: Optimized PyO3 bindings.

**Key Performance Innovation**: Ryx uses a **Zero-Allocation Row View** system. Instead of creating a Python dictionary for every row, we use a shared column mapping and a flat value vector, drastically reducing heap allocations and GC pressure during large fetches.
 
## Performance
 
Benchmark of 1 000 rows on SQLite (lower is better):
 
| Operation | Ryx ORM | SQLAlchemy ORM | SQLAlchemy Core | Ryx raw |
|-----------|--------:|---------------:|----------------:|--------:|
| **bulk_create** | 0.0074 s | 0.1696 s | 0.0022 s | 0.0011 s |
| **bulk_update** | 0.0023 s | 0.0018 s | 0.0010 s | 0.0005 s |
| **bulk_delete** | 0.0005 s | 0.0012 s | 0.0009 s | 0.0004 s |
| **filter + order + limit** | 0.0009 s | 0.0019 s | 0.0008 s | 0.0004 s |
| **aggregate** | 0.0002 s | 0.0015 s | 0.0005 s | 0.0001 s |
 
Ryx ORM is **16× faster** than SQLAlchemy ORM on bulk inserts and **2× faster** on deletes — while keeping the same Django-style API. The raw SQL layer (`raw_execute` / `raw_fetch`) gives you near-C speed when you need it.

**Internal Compilation Speed**: Our query compiler is blindingly fast, with simple lookups compiled in **~248ns** and complex query trees in **~1µs**.
 
Run the benchmark yourself:


## Documentation

Full documentation with guides, API reference, and examples: **[docs](https://ryx.alldotpy.com)**

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup, architecture details, and contribution guidelines.

## License

Python code: MIT · Rust code: MIT OR Apache-2.0
