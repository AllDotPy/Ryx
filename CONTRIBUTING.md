# Contributing to Ryx

Developer documentation, architecture details, and contribution guidelines.

## Development Setup

### Prerequisites

- **Rust 1.83+** — `rustup update stable`
- **Python 3.10+**
- **maturin** — `pip install maturin`
- **pytest + pytest-asyncio** — `pip install pytest pytest-asyncio`

### Install

```bash
git clone https://github.com/AllDotPy/Ryx
cd Ryx
maturin develop            # compile Rust + install in dev mode
```

### Run Tests
 
```bash
# Rust unit tests (no DB needed)
cargo test
 
# Python unit tests (no DB needed)
python test.py
 
# Integration tests (SQLite)
python test.py --integration
 
# All tests
python test.py --all
```
 
### Run Benchmarks
 
To measure the performance of the query compiler:
 
```bash
cd ryx-query && cargo bench
```
 
### Type Check


```bash
mypy ryx/
```

## Project Structure

```
Ryx/
├── Cargo.toml                  # Rust dependencies
├── pyproject.toml              # maturin build config
├── Makefile                    # dev shortcuts (dev, build, test, clean)
│
├── src/                        # RUST CORE (compiled to ryx_core.so)
│   ├── lib.rs                  # PyO3 module entry, QueryBuilder, type bridges
│   ├── errors.rs               # RyxError enum + PyErr conversion
│   ├── pool.rs                 # Global sqlx AnyPool singleton
│   ├── executor.rs             # SELECT/INSERT/UPDATE/DELETE execution
│   ├── transaction.rs          # Transaction handle (BEGIN/COMMIT/SAVEPOINT)
│   └── query/
│       ├── ast.rs              # QueryNode, QNode, AggregateExpr, JoinClause
│       ├── compiler.rs         # AST → SQL string + bound values
│       └── lookup.rs           # Built-in + custom lookup registry
│
├── ryx/                        # PYTHON PACKAGE
│   ├── __init__.py             # Public API surface
│   ├── __main__.py             # CLI (python -m ryx)
│   ├── models.py               # Model + ModelMetaclass + Manager + Options
│   ├── queryset.py             # QuerySet · Q · aggregates · sync/async helpers
│   ├── fields.py                # 30+ field types with validators
│   ├── validators.py           # 12 validators + run_full_validation
│   ├── signals.py              # Signal class · @receiver decorator · 8 built-in signals
│   ├── transaction.py          # TransactionContext async context manager
│   ├── relations.py            # select_related · prefetch_related
│   ├── descriptors.py          # ForwardDescriptor · ReverseFKDescriptor · M2MDescriptor
│   ├── exceptions.py           # RyxError hierarchy
│   ├── bulk.py                 # bulk_create · bulk_update · bulk_delete · stream
│   ├── cache.py                # Pluggable cache layer (MemoryCache, CachedQueryMixin)
│   ├── executor_helpers.py     # raw_fetch · raw_execute (low-level escape hatch)
│   ├── pool_ext.py             # execute_with_params · fetch_with_params
│   └── migrations/
│       ├── state.py            # SchemaState · diff engine
│       ├── runner.py           # MigrationRunner (apply)
│       ├── ddl.py              # DDLGenerator (backend-aware)
│       └── autodetect.py       # Autodetector + migration file writer
│
├── tests/
│   ├── conftest.py             # Shared fixtures, mock_core, test models
│   ├── test_compiler.rs        # 40+ Rust compiler unit tests
│   └── unit/ + integration/    # Python test suites
│
└── examples/                   # 9 progressive example scripts
```

## Architecture Deep Dive

### Data Flow (Query Execution)

```
Python: Post.objects.filter(active=True).order_by("-views").limit(10)
    │
    ▼
QuerySet.filter() → builder.add_filter("active", "exact", True, negated=False)
    │
    ▼
QuerySet.order_by() → builder.add_order_by("-views")
    │
    ▼
await queryset  →  QuerySet._execute()
    │
    ▼
PyQueryBuilder.fetch_all()  (Rust side)
    │
    ▼
compiler::compile(&QueryNode)  →  CompiledQuery { sql, values }
    │                              SELECT * FROM "posts" WHERE "active" = ?
    │                                 ORDER BY "views" DESC LIMIT 10
    ▼
executor::fetch_all(compiled)  →  sqlx::query(sql).bind(values).fetch_all(pool)
    │
    ▼
decode_row(AnyRow)  →  HashMap<String, JsonValue>
    │
    ▼
json_to_py()  →  PyDict
    │
    ▼
Model._from_row(row)  →  Model instances
```

### Key Architectural Decisions

1. **Immutable builder pattern** — Every `QuerySet` method returns a **new** QuerySet (never mutates self). The Rust `QueryNode` builder methods use `#[must_use]` and `self` (not `&mut self`) for the same immutability guarantee.

2. **AnyPool over typed pools** — Uses `sqlx::any::AnyPool` for a single code path across Postgres/MySQL/SQLite. Loses compile-time query checking but gains runtime flexibility.

3. **GIL minimization** — Rust executor decodes rows to `HashMap<String, JsonValue>` first, then converts to `PyDict` only at the PyO3 boundary. This avoids holding the GIL during SQL execution.

4. **ContextVar transaction propagation** — Active transactions are stored in `contextvars.ContextVar` so they propagate through async call stacks without explicit passing.

5. **Two-tier lookup registry** — Built-in lookups are static `fn` pointers (fast, thread-safe). Custom lookups store pre-rendered SQL templates with `{col}` placeholders. Custom lookups can override built-ins (checked first).

6. **Deferred reverse FK resolution** — ForeignKey fields with string forward references accumulate in `_pending_reverse_fk` and are resolved after each model class is defined by the metaclass.

### Dependency Versions

| Crate | Version | Role |
|---|---|---|
| `pyo3` | `>=0.28.3` | Python ↔ Rust bindings |
| `pyo3-async-runtimes` | `0.28` | Rust futures → Python awaitables |
| `sqlx` | `0.8.6` | Async SQL driver (AnyPool) |
| `tokio` | `1.40` | Async runtime |
| `thiserror` | `2` | Error type derivation |

## Rust Core Details

### `src/lib.rs` — PyO3 Module Entry

- Defines `QueryBuilder` (`PyQueryBuilder`) exposed to Python
- Setup function for module registration
- Type conversion bridges: `py_to_sql_value`, `json_to_py`
- Transaction handles exposed to Python
- Initializes the tokio runtime and lookup registry

### `src/errors.rs` — Error System

Unified `RyxError` enum with automatic Python exception conversion:

| Variant | Python Exception |
|---|---|
| `Database` | `DatabaseError` |
| `DoesNotExist` | `DoesNotExist` |
| `MultipleObjectsReturned` | `MultipleObjectsReturned` |
| `PoolNotInitialized` | `PoolNotInitialized` |
| `PoolAlreadyInitialized` | `PoolAlreadyInitialized` |
| `UnknownLookup` | `FieldError` |
| `UnknownField` | `FieldError` |
| `TypeMismatch` | `TypeError` |
| `Internal` | `RuntimeError` |

### `src/pool.rs` — Connection Pool

Global `OnceLock<AnyPool>` singleton with `PoolConfig` for tuning:

```rust
struct PoolConfig {
    max_connections: u32,
    min_connections: u32,
    connect_timeout: Duration,
    idle_timeout: Duration,
    max_lifetime: Duration,
}
```

Functions: `initialize()`, `get()`, `is_initialized()`, `stats()`.

### `src/executor.rs` — SQL Execution

- `fetch_all` — returns `Vec<HashMap<String, JsonValue>>`
- `fetch_count` — returns `i64`
- `fetch_one` — raises `DoesNotExist` / `MultipleObjectsReturned` as needed
- `execute` — INSERT/UPDATE/DELETE with `MutationResult { rows_affected, last_insert_id }`
- Transaction-aware: checks for active tx before using pool

### `src/transaction.rs` — Transaction Management

`TransactionHandle` wrapping `sqlx::Transaction<Any>`:

- `begin()`, `commit()`, `rollback()`
- `savepoint(name)`, `rollback_to(name)`, `release_savepoint(name)`
- Global `ACTIVE_TX` OnceCell for context propagation across async tasks

### `src/query/ast.rs` — Query AST

- `SqlValue` enum (Null, Bool, Int, Float, Text, Bytes, Date, Time, DateTime, Json)
- `QNode` tree (Leaf / And / Or / Not)
- `JoinClause` (Inner, LeftOuter, RightOuter, FullOuter, Cross)
- `AggFunc` (Count, Sum, Avg, Min, Max, Raw)
- `QueryNode` with builder-pattern `#[must_use]` immutable methods

### `src/query/compiler.rs` — SQL Compiler

Compiles `QueryNode` to `CompiledQuery { sql, values }`:

- SELECT, AGGREGATE, COUNT, DELETE, UPDATE, INSERT
- JOINs, WHERE (flat + Q-tree), GROUP BY, HAVING
- ORDER BY, LIMIT/OFFSET, DISTINCT
- Identifier quoting (`"col"`), LIKE wrapping for contains/startswith/endswith

### `src/query/lookup.rs` — Lookup Registry

Two-tier design:

- **Built-in** (13 lookups): `exact`, `gt`, `gte`, `lt`, `lte`, `contains`, `icontains`, `startswith`, `istartswith`, `endswith`, `iendswith`, `isnull`, `in`, `range`
- **Custom**: user-registered SQL templates with `{col}` placeholder
- Thread-safe via `RwLock`

## Python Package Details

### `ryx/__init__.py` — Public API

Exposes 70+ names via `__all__`:

- `setup()` — pool initialization with optional tuning
- `register_lookup()` / `available_lookups()` / `lookup()` decorator
- `is_connected()` / `pool_stats()`
- All model, field, queryset, signal, and exception classes

### `ryx/models.py` — Model System

- **`Options`** — model metadata (table_name, ordering, indexes, constraints, etc.)
- **`Manager`** — default query manager with 20+ proxy methods
- **`ModelMetaclass`** — processes class definitions: collects fields, adds implicit AutoField PK, injects DoesNotExist/MultipleObjectsReturned, attaches Manager, resolves pending reverse FKs
- **`Model`** — base class with hooks (`clean`, `before_save`, `after_save`, `before_delete`, `after_delete`), `full_clean()`, `save()`, `delete()`, `refresh_from_db()`

### `ryx/fields.py` — 30+ Field Types

Base `Field` with descriptor protocol (`__get__`/`__set__`), validator building, `to_python`/`to_db` conversion, `deconstruct()` for migrations.

| Integer | Text | Date/Time | Special | Relations |
|---|---|---|---|---|
| AutoField | CharField | DateField | UUIDField | ForeignKey |
| BigAutoField | SlugField | DateTimeField | JSONField | OneToOneField |
| SmallAutoField | EmailField | TimeField | ArrayField | ManyToManyField |
| IntField | URLField | DurationField | BinaryField | |
| SmallIntField | TextField | | BooleanField | |
| BigIntField | IPAddressField | | DecimalField | |
| PositiveIntField | | | NullBooleanField | |
| | | | FloatField | |

### `ryx/queryset.py` — QuerySet

Lazy, async, chainable query builder:

- `filter()`, `exclude()`, `all()`, `annotate()`, `aggregate()`
- `values()`, `join()`, `select_related()`, `order_by()`
- `limit()`, `offset()`, `distinct()`, `cache()`, `stream()`
- `using()`, `get()`, `first()`, `last()`, `exists()`, `count()`
- `delete()`, `update()`, `in_bulk()`
- Sync/async bridge (`sync_to_async`, `async_to_sync`, `run_sync`, `run_async`)
- Slice support (`qs[:3]`, `qs[2:5]`, `qs[3]`)
- Async iteration (`async for`)

### `ryx/validators.py` — Validation System

12 validators: `FunctionValidator`, `NotNullValidator`, `NotBlankValidator`, `MaxLengthValidator`, `MinLengthValidator`, `MinValueValidator`, `MaxValueValidator`, `RangeValidator`, `RegexValidator`, `EmailValidator`, `URLValidator`, `ChoicesValidator`, `UniqueValueValidator`.

`run_full_validation()` collects ALL errors from all fields before raising.

### `ryx/signals.py` — Observer Pattern

`Signal` class with `connect()` (weak references), `disconnect()`, `send()` (concurrent execution).

8 built-in signals:

| Signal | When | Kwargs |
|---|---|---|
| `pre_save` | Before INSERT/UPDATE | `instance`, `created` |
| `post_save` | After INSERT/UPDATE | `instance`, `created` |
| `pre_delete` | Before DELETE | `instance` |
| `post_delete` | After DELETE | `instance` |
| `pre_update` | Before bulk `.update()` | `queryset`, `fields` |
| `post_update` | After bulk `.update()` | `queryset`, `updated_count`, `fields` |
| `pre_bulk_delete` | Before bulk `.delete()` | `queryset` |
| `post_bulk_delete` | After bulk `.delete()` | `queryset`, `deleted_count` |

### `ryx/transaction.py` — Transaction Context

Async context manager with nesting support (outer = BEGIN, inner = SAVEPOINT). Uses `contextvars.ContextVar` for async task propagation. Auto-commit on clean exit, auto-rollback on exception.

### `ryx/relations.py` — Eager Loading

- `apply_select_related()` — LEFT JOIN + single query + row reconstruction
- `apply_prefetch_related()` — N+1 turned into 2 queries via `pk__in`

### `ryx/descriptors.py` — Attribute-Level Relation Access

- `ForwardDescriptor` — lazy-loaded FK with instance caching
- `ReverseFKManager` — QuerySet-like manager pre-filtered to parent pk
- `ManyToManyManager` — all/add/remove/set/clear/count/exists via join table

### `ryx/bulk.py` — Bulk Operations

- `bulk_create()` — multi-row INSERT with batching
- `bulk_update()` — individual UPDATEs in transactions
- `bulk_delete()` — DELETE ... WHERE pk IN
- `stream()` — async generator with LIMIT/OFFSET pagination

Bypasses per-instance hooks for performance.

### `ryx/cache.py` — Pluggable Query Cache

- `AbstractCache` protocol for custom backends
- `MemoryCache` — LRU with TTL, asyncio.Lock
- `configure_cache()`, `get_cache()`, `make_cache_key()` (SHA-256 of SQL+values)
- `CachedQueryMixin` — dynamically mixed into QuerySet
- Auto-invalidation via post_save/post_delete signals

### `ryx/migrations/` — Migration System

| Module | Responsibility |
|---|---|
| `state.py` | `ColumnState`, `TableState`, `SchemaState`, set-based diff engine |
| `ddl.py` | `DDLGenerator` — backend-aware (PG/MySQL/SQLite), type translation |
| `runner.py` | `MigrationRunner` — introspect DB, diff, generate DDL, execute |
| `autodetect.py` | `Autodetector` — compare applied state to models, generate migration files |

## Database Backends

Enable via Cargo features:

```toml
[features]
default  = ["postgres"]
postgres = ["sqlx/postgres"]
mysql    = ["sqlx/mysql"]
sqlite   = ["sqlx/sqlite"]
```

```bash
maturin develop --features postgres,sqlite
```

| URL prefix | Backend | Notes |
|---|---|---|
| `postgres://` | PostgreSQL | Full feature support |
| `mysql://` / `mariadb://` | MySQL/MariaDB | No native UUID type |
| `sqlite:///path` | SQLite (file) | No ALTER COLUMN |
| `sqlite::memory:` | SQLite (RAM) | Great for tests |

## CLI Reference

```bash
# Apply migrations
python -m ryx migrate --url postgres://... --models myapp.models

# Generate migrations
python -m ryx makemigrations --models myapp.models --dir migrations/

# Preview SQL only
python -m ryx makemigrations --models myapp.models --check   # exit 1 if changes

# Show migration status
python -m ryx showmigrations --url postgres://... --dir migrations/

# Print SQL for a migration
python -m ryx sqlmigrate 0001_initial --dir migrations/

# Delete all rows (DANGEROUS)
python -m ryx flush --models myapp.models --url postgres://... --yes

# Interactive shell with ORM pre-loaded
python -m ryx shell --url postgres://... --models myapp.models

# Connect to DB with native CLI
python -m ryx dbshell --url postgres://user:pass@localhost/mydb

# Introspect existing DB and generate model stubs
python -m ryx inspectdb --url postgres://...
python -m ryx inspectdb --url postgres://... --table users

# Version
python -m ryx version
```

CLI reads config from flags, `RYX_DATABASE_URL` env var, or `ryx_settings.py` module.

## Exception Hierarchy

```
RyxError
├── DatabaseError          # SQL / driver errors
├── PoolNotInitialized     # ryx.setup() not called
├── DoesNotExist           # .get() found nothing
├── MultipleObjectsReturned# .get() found >1
├── FieldError             # unknown field in query
└── ValidationError        # field / model validation
    .errors: dict[str, list[str]]
```

Each model also defines its own `Model.DoesNotExist` and `Model.MultipleObjectsReturned` for specific catching.

## Naming Conventions

- **Table names**: CamelCase → snake_case plural (`Post` → `posts`)
- **FK columns**: `{field_name}_id` (`author` → `author_id`)
- **Join tables**: `{model_a}_{model_b}` or user-specified via `through=`
- **Migration files**: `NNNN_description.py` (auto-numbered)

## Coding Conventions

- All code comments must be in **English**
- Every public struct, function, and class needs a doc comment explaining **what** it does and **why** it was designed that way
- Python: `from __future__ import annotations` everywhere, type hints on all signatures, `TYPE_CHECKING` guards for circular imports
- Rust: `thiserror` for error derivation, `tracing` for structured logging, `#[instrument]` on executor functions

## Roadmap

### Completed

- [x] Core query engine (SELECT, INSERT, UPDATE, DELETE)
- [x] Q objects (OR / NOT / nested)
- [x] Aggregations (COUNT, SUM, AVG, MIN, MAX, GROUP BY, HAVING)
- [x] JOINs (INNER, LEFT, RIGHT, FULL, CROSS)
- [x] Transactions + SAVEPOINTs
- [x] Validation (field-level + model-level)
- [x] Signals (pre/post save/delete/update)
- [x] Per-instance hooks
- [x] 30+ field types with full options
- [x] Backend-aware DDL generator
- [x] Migration autodetector + file writer
- [x] CLI (`python -m ryx`)
- [x] Sync/async bridge helpers
- [x] select_related / prefetch_related
- [x] Query caching layer

### Planned

- [ ] select_related via automatic JOIN reconstruction
- [ ] Reverse FK accessors (`author.posts.all()`)
- [ ] ManyToMany join table queries
- [ ] Database connection routing (multi-db)
- [ ] Streaming large result sets (`async for row in qs`)
- [ ] Bulk insert optimization (batch INSERT)
- [ ] Connection health checks / auto-reconnect
