# Ryx ORM — Examples

A collection of well-commented examples demonstrating every feature of the Ryx ORM.

## Prerequisites

```bash
# Install dependencies
uv sync

# Build the Rust extension (if not already built)
uv run maturin develop
```

## Running Examples

Each example is self-contained and can be run independently:

```bash
uv run python examples/01_setup_and_models.py
uv run python examples/02_crud_operations.py
# ... etc
```

> **Note:** Examples use SQLite by default. Each example creates its own tables with an `exN_` prefix to avoid conflicts. A shared `ryx_examples.sqlite3` file is created in the project root.

## Example Index

| # | File | Topic | Key Features |
|---|------|-------|-------------|
| 01 | `01_setup_and_models.py` | **Setup & Model Definitions** | `ryx.setup()`, Model classes, field types, Meta options, abstract models, custom PKs, Index, Constraint |
| 02 | `02_crud_operations.py` | **CRUD Operations** | `.create()`, `.save()`, `.get()`, `.first()`, `.last()`, `.all()`, `.filter()`, `.count()`, `.exists()`, `get_or_create`, `update_or_create`, `.delete()`, `refresh_from_db` |
| 03 | `03_querying_and_filters.py` | **Querying & Filters** | All lookups (`exact`, `gt`, `gte`, `lt`, `lte`, `contains`, `icontains`, `startswith`, `in`, `range`, `isnull`), Q objects (AND, OR, NOT, nesting), `exclude()`, `order_by()`, slicing pagination, `distinct()`, custom `@lookup` decorator |
| 04 | `04_aggregation_and_annotation.py` | **Aggregation & Annotation** | `Count`, `Sum`, `Avg`, `Min`, `Max`, `.aggregate()`, `.annotate()`, `.values()` + GROUP BY, distinct aggregates, `RawAgg` |
| 05 | `05_relationships_and_joins.py` | **Relationships & JOINs** | ForeignKey forward access, reverse FK manager, `.join()`, many-to-many through tables, `on_delete` behaviors |
| 06 | `06_bulk_operations.py` | **Bulk Operations** | `bulk_create`, `bulk_update`, `bulk_delete`, `QuerySet.bulk_delete()`, `.stream()`, performance comparison |
| 07 | `07_transactions.py` | **Transactions** | `async with transaction()`, commit/rollback, nested transactions (SAVEPOINTs), explicit savepoints, `get_active_transaction()`, atomic bank transfer pattern |
| 08 | `08_validation_and_clean.py` | **Validation & Clean** | Field validators, `MaxLengthValidator`, `EmailValidator`, `RegexValidator`, `ChoicesValidator`, `FunctionValidator`, custom validators, `model.clean()`, `full_clean()`, `save(validate=False)`, `ValidationError` formats |
| 09 | `09_signals.py` | **Signals** | `@receiver` decorator, `pre_save`, `post_save`, `pre_delete`, `post_delete`, `pre_update`, `post_update`, `pre_bulk_delete`, `post_bulk_delete`, custom signals, `Signal.connect()`/`disconnect()` |
| 10 | `10_caching.py` | **Caching** | `MemoryCache`, `configure_cache()`, `QuerySet.cache()`, named cache keys, auto-invalidation, `invalidate()`, `invalidate_model()`, `invalidate_all()`, TTL |
| 11 | `11_migrations.py` | **Migrations** | `MigrationRunner`, `Autodetector`, `DDLGenerator`, `detect_backend()`, `SchemaState`, `diff_states()`, schema evolution, `Meta.managed=False` |
| 12 | `12_sync_bridge.py` | **Sync/Async Bridge** | `run_sync()`, `sync_to_async()`, `async_to_sync()`, `run_async()`, CLI script pattern, sync repository pattern |

## Quick Reference

### Setup
```python
import ryx
await ryx.setup("sqlite://db.sqlite3")
```

### Define a Model
```python
from ryx import Model, CharField, IntField, DateTimeField, ForeignKey

class Author(Model):
    name = CharField(max_length=100)
    email = CharField(max_length=200, unique=True)

class Post(Model):
    title = CharField(max_length=200)
    views = IntField(default=0)
    author = ForeignKey(Author, on_delete="CASCADE")
    created_at = DateTimeField(auto_now_add=True)
```

### Run Migrations
```python
from ryx.migrations import MigrationRunner
runner = MigrationRunner([Author, Post])
await runner.migrate()
```

### CRUD
```python
# Create
author = await Author.objects.create(name="Alice", email="alice@example.com")
post = await Post.objects.create(title="Hello", author=author)

# Read
post = await Post.objects.get(pk=1)
posts = await Post.objects.filter(views__gte=100).order_by("-views")
count = await Post.objects.count()

# Update
post.title = "Hello World"
await post.save()
await Post.objects.filter(views=0).update(views=1)

# Delete
await post.delete()
await Post.objects.filter(views=0).delete()
```

### Queries
```python
from ryx import Q

# Lookups
await Post.objects.filter(title__icontains="python")
await Post.objects.filter(views__range=(10, 100))
await Post.objects.filter(id__in=[1, 2, 3])

# Q objects
await Post.objects.filter(Q(active=True) | Q(featured=True))
await Post.objects.filter(~Q(status="draft"))

# Pagination
await Post.objects.order_by("-views")[:10]
await Post.objects.order_by("-views")[10:20]
```

### Transactions
```python
from ryx import transaction

async with transaction():
    await Author.objects.create(name="Alice")
    await Post.objects.create(title="Hello", author=alice)
    # Auto-commits on exit, auto-rolls back on exception
```

### Bulk Operations
```python
# Bulk create
posts = [Post(title=f"Post {i}") for i in range(100)]
await Post.objects.bulk_create(posts)

# Streaming
async for post in Post.objects.filter(active=True).stream(chunk_size=50):
    process(post)
```

### Signals
```python
from ryx import receiver, post_save

@receiver(post_save, sender=Post)
async def notify_on_new_post(sender, instance, created, **kwargs):
    if created:
        print(f"New post: {instance.title}")
```

### Caching
```python
from ryx import MemoryCache, configure_cache

configure_cache(MemoryCache(max_size=1000, ttl=300))
posts = await Post.objects.filter(active=True).cache(ttl=60)
```

## Database Backends

Examples use SQLite by default. To use PostgreSQL or MySQL, change the `DATABASE_URL`:

```python
# PostgreSQL
DATABASE_URL = "postgres://user:pass@localhost/mydb"

# MySQL
DATABASE_URL = "mysql://user:pass@localhost/mydb"
```

Make sure to set `os.environ["RYX_DATABASE_URL"] = DATABASE_URL` so the migration runner detects the correct backend.
