<p align="center">
  <img src="https://github.com/AllDotPy/Ryx/blob/master/logo.svg?raw=true" alt="Ryx ORM" width="80" height="80" />
</p>

<h1 align="center">Ryx ORM</h1>

<p align="center">
  <strong>Django-style ORM — Python and Rust. Powered by Rust.</strong>
</p>

<p align="center">
  <a href="https://pypi.org/project/ryx/"><img src="https://img.shields.io/badge/python-3.10%2B-blue?style=for-the-badge&logo=python&logoColor=white" alt="Python 3.10+" /></a>
  <a href="https://pypi.org/project/ryx/"><img src="https://img.shields.io/pypi/v/ryx?style=for-the-badge" alt="Version" /></a>
  <a href="https://github.com/AllDotPy/Ryx/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-MIT-green?style=for-the-badge" alt="License" /></a>
  <a href="https://discord.gg/umDhd5HWgS"><img src="https://img.shields.io/discord/1452761060678303909?style=flat-square&logo=discord" alt="Discord" /></a>
</p>

---

## Install

```bash
pip install ryx
```

## Quick Start

```python
import ryx
from ryx import Model, CharField, IntField, BooleanField, Q

class Post(Model):
    title  = CharField(max_length=200)
    views  = IntField(default=0)
    active = BooleanField(default=True)

await ryx.setup("postgres://user:pass@localhost/mydb")
posts = await Post.objects.filter(Q(active=True) | Q(views__gte=1000))
```

## Documentation

Full docs, guides and API reference: **[ryx.alldotpy.com](https://ryx.alldotpy.com)**

- [Python quick start](https://ryx.alldotpy.com/getting-started/quick-start)
- [Rust quick start](https://ryx.alldotpy.com/rust/getting-started/quick-start)

## Features

- **Django-style API** — `Model`, `Manager`, lazy chainable `QuerySet`
- **Q objects** — `OR` / `AND` / `NOT` with 30+ field lookups
- **Relationships** — ForeignKey, OneToOne, ManyToMany, `select_related`
- **Migrations** — built-in autodetection and schema diffing
- **Multi-database** and **PostgreSQL multi-schema** support
- **Vector search** via pgvector (`VectorField`, K-NN)
- **File/Image fields** with pluggable storage backends
- **Backends** — PostgreSQL · MySQL · SQLite

## License

Python code: MIT · Rust code: MIT OR Apache-2.0
