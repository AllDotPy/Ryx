<p align="center">
  <img src="logo.svg" alt="Ryx ORM" width="80" height="80" />
</p>

<h1 align="center">Ryx ORM</h1>

<p align="center">
  <strong>Django-style Python ORM. Powered by Rust.</strong>
</p>

<p align="center">
  <a href="https://pypi.org/project/ryx/"><img src="https://img.shields.io/badge/python-3.10%2B-blue?style=for-the-badge&logo=python&logoColor=white" alt="Python 3.10+" /></a>
  <a href="https://pypi.org/project/ryx/"><img src="https://img.shields.io/pypi/dm/ryx?style=for-the-badge&logo=pypi&logoColor=white&label=downloads" alt="PyPI Downloads" /></a>
  <!-- <a href="https://pepy.tech/projects/ryx"><img src="https://static.pepy.tech/badge/ryx?style=for-the-badge" alt="Total Downloads" /></a> -->
  <a href="https://github.com/AllDotPy/Ryx/releases"><img src="https://img.shields.io/badge/v0.1.0--beta-orange?style=for-the-badge" alt="Version" /></a>
  <a href="https://github.com/AllDotPy/Ryx/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-AGPL--3.0-green?style=for-the-badge" alt="License" /></a>
  <a href="https://github.com/rust-lang/rust"><img src="https://img.shields.io/badge/rust-1.83%2B-orange?style=for-the-badge&logo=rust" alt="Rust 1.83+" /></a>
</p>

<p align="center">
  <a href="https://github.com/AllDotPy/Ryx/stargazers"><img src="https://img.shields.io/github/stars/AllDotPy/Ryx?style=social" alt="GitHub stars" /></a>
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

## Performance

Benchmark of 1 000 rows on SQLite (lower is better):

| Operation | Ryx ORM | SQLAlchemy ORM | SQLAlchemy Core | Ryx raw |
|-----------|--------:|---------------:|----------------:|--------:|
| **bulk_create** | 0.007 s | 0.121 s | 0.002 s | 0.001 s |
| **bulk_update** | 0.003 s | 0.001 s | 0.001 s | 0.001 s |
| **bulk_delete** | 0.001 s | 0.001 s | 0.001 s | 0.001 s |
| **filter + order + limit** | 0.001 s | 0.002 s | 0.001 s | 0.000 s |
| **aggregate** | 0.000 s | 0.001 s | 0.001 s | 0.000 s |

Ryx ORM is **16× faster** than SQLAlchemy ORM on bulk inserts and **2× faster** on deletes — while keeping the same Django-style API. The raw SQL layer (`raw_execute` / `raw_fetch`) gives you near-C speed when you need it.

Run the benchmark yourself:

```bash
uv add sqlalchemy[asyncio] aiosqlite
uv run python examples/13_benchmark_sqlalchemy.py
```

## Quick Start

```bash
pip install maturin
maturin develop          # compile Rust + install
```

```python
import asyncio, ryx
from ryx import Model, CharField

class Article(Model):
    title = CharField(max_length=200)

async def main():
    await ryx.setup("sqlite:///app.db")
    await ryx.migrate([Article])
    await Article.objects.create(title="Hello Ryx")
    print(await Article.objects.all())

asyncio.run(main())
```

## Key Features

- **30+ field types** — from `AutoField` to `JSONField`, with validation built in
- **Q objects** — complex `AND` / `OR` / `NOT` expressions with nesting
- **Aggregations** — `Count`, `Sum`, `Avg`, `Min`, `Max` with `GROUP BY` and `HAVING`
- **Relationships** — `ForeignKey`, `OneToOneField`, `ManyToManyField` with `select_related` / `prefetch_related`
- **Transactions** — async context managers with nested savepoints
- **Signals** — `pre_save`, `post_save`, `pre_delete`, `post_delete` and more
- **Migrations** — autodetect schema changes, generate and apply
- **Validation** — field-level + model-level, collects all errors before raising
- **Sync/async bridge** — use from sync or async code seamlessly
- **CLI** — `python -m ryx migrate`, `makemigrations`, `shell`, `inspectdb`

## Architecture

<p align="center">
  <img src="ryx_architecture.svg" alt="Ryx Architecture" width="100%" />
</p>

Your Python queries are compiled to SQL in Rust, executed by sqlx, and decoded back — all without blocking the Python event loop.

## Documentation

Full documentation with guides, API reference, and examples: **[docs](https://ryx.alldotpy.com)**

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup, architecture details, and contribution guidelines.

## License

Python code: AGPL-3.0 · Rust code: MIT OR Apache-2.0
