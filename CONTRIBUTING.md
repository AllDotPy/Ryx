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
maturin develop            # compile Rust workspace + install in dev mode
```

### Run Tests

```bash
# Rust unit tests (across all workspace crates)
cargo test

# Python unit tests
python test.py

# Integration tests (SQLite)
python test.py --integration

# All tests
python test.py --all
```

### Run Benchmarks

To measure the performance of the query compiler:

```bash
cargo bench -p ryx-query
```

### Type Check

```bash
mypy ryx/
```

## Project Structure

Ryx uses a Rust Workspace to isolate core logic, backend implementations, and Python bindings.

```
Ryx/
├── Cargo.toml                  # Workspace configuration
├── pyproject.toml              # maturin build config
├── Makefile                    # dev shortcuts
│
├── ryx-core/                   # CORE TYPES & TRAITS
│   └── src/                    # Connection/Transaction enums, Base types
│
├── ryx-backend/                # DATABASE ADAPTERS
│   └── src/                    # Executor implementations, Row decoding
│
├── ryx-query/                  # SQL COMPILER
│   └── src/                    # AST, Compiler, Lookup registry
│
├── ryx-python/                 # PyO3 BINDINGS
│   └── src/                    # Module entry, Type bridges, Bound object handling
│
├── ryx/                        # PYTHON PACKAGE
│   ├── __init__.py             # Public API surface
│   ├── models.py               # Model, Metaclass, Manager
│   ├── queryset.py             # Lazy QuerySet implementation
│   ├── fields.py               # Field types and validators
│   └── ... (other python modules)
│
├── tests/                      # Test suites
└── examples/                   # Usage examples
```

## Architecture Deep Dive

### Performance Philosophy

Ryx is designed for extreme performance, targeting 1-2 $\mu$s overhead for query construction and row decoding.

1. **Enum Dispatch**: We avoid `dyn` traits and vtable lookups in the hot path. `RyxConnection` and `RyxTransaction` are enums that allow the compiler to inline backend-specific logic.
2. **Zero-Allocation Rows**: Instead of creating a `HashMap` for every database row, we use `RowView` and `RowMapping`. A single mapping is shared across all rows in a result set, and values are accessed via index.
3. **GIL Minimization**: Data is decoded into optimized Rust structures before being converted to Python objects at the very last moment, minimizing the time the GIL is held.

### Data Flow (Query Execution)

```
Python: Post.objects.filter(active=True).limit(10)
    │
    ▼
QuerySet → PyQueryBuilder (ryx-python)
    │
    ▼
compiler::compile (ryx-query) → CompiledQuery { sql, values }
    │
    ▼
executor::fetch_all (ryx-backend) → sqlx::query(sql).fetch_all(pool)
    │
    ▼
decode_rows (ryx-backend) → Vec<RowView> (Zero-allocation)
    │
    ▼
RowView → PyDict (ryx-python)
    │
    ▼
Model._from_row(row) → Model instances
```

## Coding Conventions

- **No Dynamic Dispatch**: Avoid `Box<dyn Trait>` in hot paths. Use enums or generics.
- **PyO3 0.28.3**: Always use `Bound<'py, T>` for Python objects. Use `cast::<_>()` for type conversions.
- **Immutability**: `QuerySet` and `QueryNode` must remain immutable. Methods should return new instances.
- **Documentation**: Every public Rust item must have a doc comment. Python signatures must have full type hints.
- **English Only**: All code and comments must be in English.
