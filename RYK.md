## Goal
- Complete multi-schema support for PostgreSQL across migration, query, and CLI layers in both Rust and Python, then add comprehensive tests.

## Constraints & Preferences
- Models are **schema-agnostic** — no `#[schema]` on models. Schema is specified at query time (`.schema("name")` on QuerySet) or migration time (`.schema("name")` on runner).
- `schema=""` (default) means the backend's default schema (public for PG, ignored for MySQL/SQLite). No qualification in SQL when unset → 100% backward compat.
- Composite key `(schema, table_name)` everywhere: `diff_states`, introspection, DDL generation.
- `CREATE SCHEMA IF NOT EXISTS` is automatic in the runner (when schema is non-empty and backend supports it).
- Schema support is **PostgreSQL only** — MySQL/SQLite treat schema field as no-op (`supports_schemas()` returns `false` for non-PG).
- Performance: `Option<Symbol>` in QueryNode (not `Option<String>`), `supports_schemas()` is a `const fn`, qualification done once per SQL compile (not per row).
- Rust `gen` is reserved in edition 2024 — use `ddl_gen` instead of `gen` as variable name.

## Progress
### Done
- **Rust — Backend.supports_schemas()** added to `ryx-query/src/backend.rs` as a `const fn`.
- **Rust — TableState.schema**, **SchemaChange.schema**, **ChangeKind::CreateSchema** — all core types updated. `diff_states()` rewritten with composite key `(schema, name)`.
- **Rust — DDLGenerator.schema** with `in_schema()` builder, `qn()` helper, `create_schema()`. All DDL methods (`create_table`, `drop_table`, `add_column`, `drop_column`, `alter_column`, `create_index`, `drop_index`, `add_check_constraint`, `add_foreign_key`) use `self.qn()`. `generate()` handles `CreateSchema` in zeroth pass.
- **Rust — Operations** (`operations.rs`): `CreateSchema` variant, `schema: String` on all table operations, `schema()` accessor.
- **Rust — Autodetect** (`autodetect.rs`): `ModelEntry.schema`, `build_target()` schema apply, `changes_to_operations()` emits `CreateSchema` + schema pass-through.
- **Rust — Runner** (`runner.rs`): `FileRunner.schema`, `.schema()` builder, schema-aware `run_live()`, `plan_live()`, `apply_files()`, schema override per operation in `apply_files()`/`plan_files()`. `operation_to_sql()` handles `CreateSchema`.
- **Rust — Introspection**: PostgreSQL uses `table_schema = '{schema}'`, MySQL/SQLite set `schema: ""`.
- **Rust — Query layer** (`ryx-query/src/ast.rs`): `QueryNode.schema` (`Option<Symbol>`), `with_schema()` builder. Compiler (`compilr.rs`): `write_table_ref()` / `write_join_table_ref()` helpers, all compile functions (`SELECT`, `COUNT`, `DELETE`, `UPDATE`, `INSERT`, `JOIN`) use schema qualification. Plan hash includes `node.schema`.
- **Rust — QuerySet** (`ryx-rs/src/queryset.rs`): `.schema()` builder method.
- **Rust — CLI** (`ryx-rs/src/main.rs`): `--schema` flag on `Migrate` and `Sqlmigrate` subcommands, passed to `FileRunner.schema()` and `DDLGenerator.in_schema()`.
- **Python — state.py**: `TableState.schema`, `ChangeKind.CREATE_SCHEMA`, `SchemaChange.schema`, `diff_states()` composite key + CreateSchema detection. `to_json()`/`from_json()` schema-aware.
- **Python — ddl.py**: `DDLGenerator.schema` parameter, `_qn()` helper, `create_schema()`. All DDL methods use `_qn()` for table references. `generate_schema_ddl()` creates per-table schema DDLGenerator.
- **Python — autodetect.py**: `schema: str` on all operation types (`CreateTable`, `AddField`, `AlterField`, `CreateIndex`), `to_python()` serialization includes schema, `apply_migration_to_state()` preserves schema on tables, `_changes_to_operations()` passes schema from changes.
- **Python — runner.py**: `_schema` on `MigrationRunner`, `.schema()` builder, `_create_ddl()` helper, schema-aware `_operation_to_ddl()`, `_ddl_for_change()`, `_introspect_schema()`, `_get_tables()`, `_get_columns()`, `_apply_meta_extras()`, `_ensure_m2m_table()`.
- **Python — query layer**: `QuerySet.schema()` method adds `("schema", name)` op to ops list. Rust `build_plan()` in `ryx-python/src/plan.rs` handles `"schema"` op via `node.with_schema()`.
- **Python — CLI**: `--schema` flag on `migrate`, `sqlmigrate`, `inspectdb` commands in `ryx-python/ryx/__main__.py`, `ryx-python/ryx/cli/commands/migrate.py`, `sqlmigrate.py`, `inspectdb.py`.
- **Rust unit tests** — **30 new multi-schema tests added** (75 unit total):
  - migration.rs: 7 multi-schema diff tests (CreateSchema detection, same table different schemas, empty/noop/mixed)
  - ddl.rs: 20 schema-qualified DDL tests (create_schema, create_table, alter, drop, index, FK, constraint, MySQL/SQLite ignore, backward compat)
  - operations.rs: 7 operation schema tests (all variants)
  - compilr.rs: 12 schema query tests (SELECT/COUNT/DELETE/UPDATE/INSERT/JOIN with schema, MySQL/SQLite ignore, plan hash differential)
- **Python unit tests** — **42 new tests added**:
  - test_migration_state.py: 11 tests (diff_states composite key, CreateSchema detection, serialization)
  - test_migration_ddl.py: 17 tests (create_schema, qualified names, all DDL methods, backward compat)
  - test_migration_autodetect.py: 10 tests (operation schema fields, to_python, apply_migration_to_state, AddField schema inheritance)
- **Rust integration test**: `ryx-rs/tests/multi_schema_test.rs` — full multi-schema pipeline: migrate tenant1, migrate tenant2, verify table isolation across schemas, insert/query data independently, cleanup. Uses `PG_TEST_URL` env var (defaults to `postgres://einswilli@localhost/ryx_integration_test`). Passes.
- **Python integration test**: `ryx-python/tests/integration/test_multi_schema.py` — same pipeline, runs in subprocess to avoid conftest's SQLite pool. Overrides `setup_database` and `clean_tables` fixtures. Passes.

### In Progress
- (none — feature complete)

### Blocked
- Python `_handle_no_migration_files` interactive prompt blocks automated migration testing (no `live` flag). Tests work around it by monkeypatching `input()`.

## Key Decisions
- **Schema at query/migration time, not on model**: Models stay agnostic → one model works in N schemas without duplication.
- **Empty string = no schema = backward compat**: All existing code continues to work unchanged. Schema qualification only activates when explicitly set.
- **`Option<Symbol>` vs `Option<String>` in QueryNode**: `Option<Symbol>` for interned performance, `None` = no qualification.
- **Composite key `(schema, name)` everywhere**: Enables correct diff across schemas. No schema = empty string matches "no schema" (no `CreateSchema` emitted).
- **`CREATE SCHEMA IF NOT EXISTS` is automatic**: Runner creates schema before tables when schema is non-empty and backend supports schemas. Not part of migration files (idempotent).
- **Per-op schema override**: Each operation can carry its own schema, allowing mixed-schema migration files (unlikely but supported).
- **Temporary DDLGenerator per change**: Used in `generate()`, `apply_files()`, `plan_files()` to switch schema context. Acceptable cost since migration ops are rare.
- **Python QuerySet delegates schema to Rust FFI**: `("schema", name)` op is stored in ops list and handled by Rust `build_plan()`. This means all SQL compilation is schema-aware with zero additional Python overhead.
- **Integration tests run in subprocess for Python**: The `ryx` package auto-initializes the pool from `ryx.toml` at import time. Python's conftest also inits a SQLite pool. PG tests must run separately via subprocess with `RYX_AUTO_INITIALIZE=0`.

## Next Steps
1. Add `live=True` flag to Python `MigrationRunner` to skip interactive prompt in tests (feature gap).
2. Documentation update: multi-schema section in migrations.mdx, `.schema()` API reference.

## Critical Context
- **Rust integration tests**: 76 unit + 2 integration (SQLite full pipeline + PG multi-schema).
- **Python integration tests**: 42 unit + 1 integration (PG multi-schema via subprocess).
- `diff_states()` matches tables by `(schema, name)` composite key. Two tables with same name in different schemas are treated as different tables. `CreateSchema` is emitted for schemas with non-empty string.
- `DDLGenerator.qn()` produces `"schema"."table"` when schema is non-empty and backend supports schemas. For MySQL/SQLite, `supports_schemas()` → `false` eliminates qualification at compile time.
- `QueryNode.with_schema(schema)` interns the schema name as `Symbol`. The compiler's `write_table_ref()` checks `node.backend.supports_schemas()` at compile time.
- Python `QuerySet.schema()` adds an op that the Rust FFI `build_plan()` converts to `node.with_schema()`. The same Rust compiler handles qualification.
- `__main__.py` has two parser code paths: the old argparse-based `_build_parser()` and the new registry-based `build_parser()`. `--schema` was added only to `_build_parser()` for now.
- `inspectdb.py` hardcodes `table_schema = 'public'` for PostgreSQL introspection (the `--schema` flag passes through to the runner layer).
- Integration test requires a running PostgreSQL on localhost:5432 with user `einswilli` (no password) and a `ryx_integration_test` database.
- Python `ryx` package has `ryx/__init__.py:_auto_setup()` that reads `ryx.toml` and initializes the pool at `import ryx` time. Set `RYX_AUTO_INITIALIZE=0` to prevent this.
- Python conftest adds `setup_database` (session-scoped, SQLite) and `clean_tables` (autouse, async) to all integration tests. PG tests must override both.

## Relevant Files
- **Rust modified**:
  - `ryx-query/src/ast.rs`: `QueryNode.schema` + `with_schema()`.
  - `ryx-query/src/compiler/compilr.rs`: `write_table_ref()`, `write_join_table_ref()`, all compile functions updated, plan hash includes schema.
  - `ryx-query/src/backend.rs`: `supports_schemas()` const fn.
  - `ryx-rs/src/migration.rs`: core types, `diff_states()`, introspection, test helpers, 7 new schema diff tests.
  - `ryx-rs/src/migration/ddl.rs`: `DDLGenerator.in_schema()`, `qn()`, `create_schema()`, `generate()` zeroth pass, 20 new DDL schema tests.
  - `ryx-rs/src/migration/operations.rs`: `CreateSchema` variant, `schema` on all ops, `schema()` accessor, 7 new operation schema tests.
  - `ryx-rs/src/migration/autodetect.rs`: `ModelEntry.schema`, `build_target()` schema, `changes_to_operations()` CreateSchema.
  - `ryx-rs/src/migration/runner.rs`: `FileRunner.schema`, `.schema()` builder, schema-aware run/plan/apply.
  - `ryx-rs/src/queryset.rs`: `.schema()` builder method.
  - `ryx-rs/src/main.rs`: `--schema` on Migrate/Sqlmigrate.
- **Python modified**:
  - `ryx-python/ryx/migrations/state.py`: `TableState.schema`, `ChangeKind.CREATE_SCHEMA`, `SchemaChange.schema`, `diff_states()` composite key, `to_json()`/`from_json()` schema-aware.
  - `ryx-python/ryx/migrations/ddl.py`: `DDLGenerator.schema`, `_qn()`, `create_schema()`, all DDL methods qualified, `generate_schema_ddl()` per-table schema.
  - `ryx-python/ryx/migrations/autodetect.py`: `schema` on all operation types, `to_python()` includes schema, `apply_migration_to_state()` preserves schema.
  - `ryx-python/ryx/migrations/runner.py`: `_schema`, `.schema()` builder, `_create_ddl()`, all internal methods schema-aware.
  - `ryx-python/ryx/queryset.py`: `.schema()` method.
  - `ryx-python/src/plan.rs`: `"schema"` op handler.
  - `ryx-python/ryx/__main__.py`: `--schema` on migrate/sqlmigrate/inspectdb.
  - `ryx-python/ryx/cli/commands/migrate.py`: `--schema` flag + pass to runner.
  - `ryx-python/ryx/cli/commands/sqlmigrate.py`: `--schema` flag + DDLGenerator schema.
  - `ryx-python/ryx/cli/commands/inspectdb.py`: `--schema` flag + schema-aware introspection.
- **Test files**:
  - `ryx-rs/src/migration/operations.rs`: 7 new schema tests.
  - `ryx-python/tests/unit/test_migration_state.py`: 11 new tests.
  - `ryx-python/tests/unit/test_migration_ddl.py`: 17 new tests.
  - `ryx-python/tests/unit/test_migration_autodetect.py`: 10 new tests.
  - `ryx-rs/tests/multi_schema_test.rs`: PG multi-schema integration test.
  - `ryx-python/tests/integration/test_multi_schema.py`: Python PG multi-schema integration test.
