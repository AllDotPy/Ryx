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

---

## 🌐 Dual-Language ORM

Ryx delivers the **same expressive Django-style ORM API** in both Python and Rust.

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

```bash
pip install ryx
```

```python
from ryx import Model, CharField, IntField, BooleanField, DateTimeField, Q

class Post(Model):
    title = CharField(max_length=200)
    views = IntField(default=0)
    active = BooleanField(default=True)
    created = DateTimeField(auto_now_add=True)

await ryx.setup("postgres://user:pass@localhost/mydb")

posts = await Post.objects.filter(
    Q(active=True) | Q(views__gte=1000),
).order_by("-views").limit(20)

stats = await Post.objects.aggregate(total=Count("id"), avg_views=Avg("views"))
```

---

## 🦀 Rust — Same API, Zero Python Dependencies

```toml
[dependencies]
ryx-rs = "0.1"
ryx-macro = "0.1"
tokio = { version = "1", features = ["full"] }
```

### Setup & Model Definition

```rust
use ryx_rs::model;

#[model]
#[table("posts")]
struct Post {
    #[field(pk)]
    id: i64,
    title: String,
    slug: String,
    views: i64,
    active: bool,
}
```

The `#[model]` attribute derives `Model`, `FromRow`, `Serialize`, and `Deserialize` in one step.

### Configuration & Migration

```rust
use std::collections::HashMap;
use ryx_rs::{init, migration::MigrationRunner, PoolConfig};

#[tokio::main]
async fn main() -> ryx_rs::RyxResult<()> {
    // Manual pool setup
    let mut urls = HashMap::new();
    urls.insert("default".into(), "sqlite:myapp.db?mode=rwc".into());
    ryx_backend::pool::initialize(urls, PoolConfig::default()).await?;

    // Or from config (ryx.toml / ryx.yaml):
    // init().await?;

    // Auto-create tables from your models
    MigrationRunner::new()
        .model::<Post>()
        .run().await?;

    // ... queries
    Ok(())
}
```

### CRUD

```rust
// INSERT
let post = Post::objects().create()
    .set("title", "Hello Ryx")
    .set("slug", "hello-ryx")
    .set("views", 42i64)
    .set("active", true)
    .save().await?;

// SELECT all
let posts: Vec<Post> = Post::objects().all().all().await?;

// FILTER
let posts: Vec<Post> = Post::objects()
    .filter("active", true)
    .filter("views__gte", 100i64)
    .all().await?;

// GET by id
let post: Post = Post::objects().get("id", 1i64).all().await?.into_iter().next().unwrap();

// FIRST
let first: Option<Post> = Post::objects()
    .filter("active", true)
    .order_by("created")
    .first().await?;

// ORDER BY + LIMIT + OFFSET
let posts: Vec<Post> = Post::objects()
    .order_by("-views")
    .limit(10)
    .offset(20)
    .all().await?;

// UPDATE
Post::objects()
    .filter("author", "bob")
    .update(vec![("views", 9999i64)])
    .await?;

// DELETE
Post::objects()
    .filter("title__startswith", "Draft")
    .delete().await?;

// COUNT
let total = Post::objects().filter("active", true).count().await?;

// EXISTS
let has_drafts = Post::objects()
    .filter("title__startswith", "Draft")
    .exists().await?;
```

### Q Objects (OR / AND / NOT)

```rust
use ryx_rs::Q;

// OR: active OR (views >= 1000)
let posts = Post::objects()
    .filter(Q::or(
        Q::new("active", true),
        Q::new("views__gte", 1000i64),
    ))
    .all().await?;

// AND + OR: (active OR draft) AND views > 0
let posts = Post::objects()
    .filter(Q::and(
        Q::or(
            Q::new("active", true),
            Q::new("draft", true),
        ),
        Q::new("views__gt", 0i64),
    ))
    .all().await?;

// NOT: NOT active
use ryx_rs::Q;
let posts = Post::objects()
    .filter(Q::not("active", true))
    .all().await?;
```

### Field Lookups (30+)

Ryx supports Django-style field lookups via the `__` separator:

| Lookup | Example | SQL |
|---|---|---|
| **exact** | `"title", "Hello"` | `= 'Hello'` |
| **gte** | `"views__gte", 100` | `>= 100` |
| **gt** | `"views__gt", 100` | `> 100` |
| **lte** | `"views__lte", 100` | `<= 100` |
| **lt** | `"views__lt", 100` | `< 100` |
| **contains** | `"title__contains", "Rust"` | `LIKE '%Rust%'` |
| **startswith** | `"title__startswith", "Draft"` | `LIKE 'Draft%'` |
| **endswith** | `"slug__endswith", "-ryx"` | `LIKE '%-ryx'` |
| **in** | `"status__in", ["a", "b"]` | `IN ('a', 'b')` |
| **isnull** | `"author__isnull", true` | `IS NULL` |
| **year / month / day** | `"created__year", 2024` | `EXTRACT(YEAR FROM ...)` |

```rust
let posts = Post::objects()
    .filter("title__contains", "Rust")
    .filter("views__gte", 100i64)
    .filter("created__year", 2024i64)
    .all().await?;
```

### Aggregations

```rust
use ryx_rs::agg::{count, sum, avg, min, max, count_distinct};

let stats = Post::objects()
    .aggregate(&[
        count("total", "id"),
        sum("total_views", "views"),
        avg("avg_views", "views"),
    ]).await?;

println!("Total posts: {:?}", stats.get("total"));
println!("Total views: {:?}", stats.get("total_views"));

// Annotate — per-row aggregations
let annotated = Post::objects()
    .annotate(&[count("comment_count", "id")])
    .await?;
for row in &annotated {
    println!("Comments: {:?}", row.get("comment_count"));
}
```

### Values / Values List

```rust
// HashMap per row
let values = Post::objects()
    .filter("active", true)
    .values(&["title", "views"])
    .await?;
for row in &values {
    println!("{}: {:?}", row.get("title").unwrap(), row.get("views").unwrap());
}

// Vec per row
let list = Post::objects()
    .values_list(&["title", "views"])
    .await?;
for cols in &list {
    println!("{:?}", cols);
}

// DISTINCT
let authors = Post::objects()
    .distinct()
    .values(&["author"])
    .await?;
```

### Relations: select_related

Define a foreign-key relationship with `#[relation(...)]`:

```rust
#[model]
#[table("authors")]
struct Author {
    #[field(pk)]
    id: i64,
    name: String,
}

#[model]
#[table("posts")]
#[relation(model = "Author", fk_column = "author_id", name = "author")]
struct Post {
    #[field(pk)]
    id: i64,
    title: String,
    author_id: i64,
    author: Option<Author>,  // populated by select_related
}
```

Fetch with a single `LEFT JOIN`:

```rust
let posts: Vec<Post> = Post::objects()
    .all()
    .select_related(&["author"])
    .all().await?;

for post in &posts {
    if let Some(author) = &post.author {
        println!("{} — by {}", post.title, author.name);
    }
}
```

The join produces aliased columns (`author__id`, `author__name`) and automatically decodes the nested model via `FromRow::from_row_prefixed`.

### Transactions

```rust
use ryx_rs::transaction;

transaction(|tx| async move {
    Post::objects().filter("author", "bob").delete().await?;
    Post::objects().create()
        .set("title", "New Post")
        .set("author_id", 1i64)
        .save().await?;
    tx.commit().await  // or tx.rollback()
}).await?;
```

### Streaming (Keyset Pagination)

```rust
let mut stream = Post::objects()
    .filter("active", true)
    .order_by("id")
    .stream(100, Some("id"));

while let Some(chunk) = stream.next_chunk().await? {
    for post in chunk {
        // process 100 posts at a time
    }
}
```

### Caching

```rust
use ryx_rs::cache::{MemoryCache, configure_cache};

configure_cache(MemoryCache::new(1000));

let posts = Post::objects()
    .filter("active", true)
    .cache(60, Some("active_posts"))  // TTL: 60s
    .all().await?;
```

### Debug: See Generated SQL

```rust
let sql = Post::objects()
    .filter("title__contains", "Rust")
    .sql()?;
println!("{}", sql);
// SELECT * FROM "posts" WHERE "title" LIKE '%Rust%'
```

---

## 📊 Comparison

| | Diesel | SeaORM | **Ryx (Rust)** |
|---|---|---|---|
| **API style** | Schema-first, macros | Verbose builders | **Django-like, concise** |
| **Learning curve** | Steep | Moderate | **Low (Django devs)** |
| **Async** | sync + async | Async | **Async (tokio)** |
| **Q objects (OR/AND/NOT)** | ❌ | ❌ | ✅ |
| **Lookups** | Basic | Basic | **30+ lookups & transforms** |
| **select_related** | ❌ | ✅ (Eager) | ✅ |
| **Aggregations** | ✅ | ✅ | ✅ |
| **Migrations** | Diesel CLI | sea-orm-cli | **Built-in** |
| **Backends** | PG · MySQL · SQLite | PG · MySQL · SQLite | **PG · MySQL · SQLite** |

---

## 🏗 Architecture

<p align="center">
   <img src="https://github.com/AllDotPy/Ryx/blob/master/ryx_architecture.svg?raw=true" alt="Ryx Architecture" width="100%" />
</p>

```
          Python (ryx-python)           Rust (ryx-rs)
                │                            │
          PyO3 bridge ────────╗       no pyo3
                │             ║           │
          ┌─────┴─────────────║───────────┴──────┐
          │      ryx-core     ║     ryx-common    │
          │  (shared types)   ║  (errors, config)  │
          └─────┬─────────────║───────────┬──────┘
                │             ║           │
          ┌─────┴─────────────║───────────┴──────┐
          │         ryx-query (SQL compiler)       │
          │   ~248ns per simple lookup compile     │
          └───────────────────┬──────────────────┘
                              │
          ┌───────────────────┴──────────────────┐
          │          ryx-backend (sqlx)            │
          │    Postgres · MySQL · SQLite           │
          └──────────────────────────────────────┘
```

---

## ⚡ Performance

Benchmark of 1 000 rows on SQLite (lower is better):

| Operation | Ryx ORM | SQLAlchemy ORM | SQLAlchemy Core | Ryx raw |
|---|---|---|---|---|
| **bulk_create** | 0.0074 s | 0.1696 s | 0.0022 s | 0.0011 s |
| **bulk_update** | 0.0023 s | 0.0018 s | 0.0010 s | 0.0005 s |
| **bulk_delete** | 0.0005 s | 0.0012 s | 0.0009 s | 0.0004 s |
| **filter + order + limit** | 0.0009 s | 0.0019 s | 0.0008 s | 0.0004 s |
| **aggregate** | 0.0002 s | 0.0015 s | 0.0005 s | 0.0001 s |

Ryx ORM is **16× faster** than SQLAlchemy ORM on bulk inserts and **2× faster** on deletes.

---

## 📚 Documentation

Full documentation with guides, API reference, and examples: **[ryx.alldotpy.com](https://ryx.alldotpy.com)**

- [Python quick start](https://ryx.alldotpy.com/python/quickstart)
- [Rust quick start](https://ryx.alldotpy.com/rust/quickstart)
- [API reference (Rust)](https://ryx.alldotpy.com/rust/api/ryx_rs)

---

## 🤝 Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup, architecture details, and contribution guidelines.

## 📄 License

Python code: MIT · Rust code: MIT OR Apache-2.0
