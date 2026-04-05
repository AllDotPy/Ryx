"""
Ryx ORM — Bulk Operations

bulk_create : INSERT many rows in a single SQL statement (or batched).
bulk_update : UPDATE many rows using a CASE expression.
bulk_delete : DELETE rows by PK list.

These bypass per-instance hooks and validation by default (for performance).
Pass validate=True to run full_clean() on each instance before inserting.

Usage:
  posts = [Post(title=f"Post {i}") for i in range(1000)]
  await bulk_create(Post, posts, batch_size=500)

  await bulk_update(Post, posts, fields=["views", "active"])

Design notes:
  - bulk_create uses a single multi-row INSERT: INSERT INTO t (a,b) VALUES (?,?),(?,?)
    which is much faster than N individual INSERTs.
  - We batch by batch_size to avoid hitting DB parameter limits (SQLite: 999,
    Postgres: 65535, MySQL: 65535).
  - bulk_update emits one UPDATE per batch using a VALUES list + JOIN trick on
    Postgres/MySQL, or a CASE WHEN expression on SQLite.
"""

from __future__ import annotations

# import asyncio
# import itertools
from typing import List, Sequence, Type, TYPE_CHECKING

if TYPE_CHECKING:
    from ryx.models import Model


def _detect_backend() -> str:
    """Detect the database backend from the RYX_DATABASE_URL env var.

    Returns one of: "sqlite", "postgres", "mysql".
    Falls back to "sqlite" if the URL cannot be parsed.
    """
    import os

    url = os.environ.get("RYX_DATABASE_URL", "").lower()
    if url.startswith("postgres://") or url.startswith("postgresql://"):
        return "postgres"
    if url.startswith("mysql://") or url.startswith("mariadb://"):
        return "mysql"
    if url.startswith("sqlite://"):
        return "sqlite"
    # Default to sqlite for local development
    return "sqlite"


####    bulk_create
async def bulk_create(
    model: Type["Model"],
    instances: Sequence["Model"],
    *,
    batch_size: int = 500,
    validate: bool = False,
    ignore_conflicts: bool = False,
) -> List["Model"]:
    """Insert many model instances in batches.

    Significantly faster than calling ``instance.save()`` in a loop because
    it uses a single multi-row ``INSERT INTO t (...) VALUES (...),(...)``
    per batch.

    Args:
        model:            The Model class.
        instances:        Sequence of unsaved model instances.
        batch_size:       Number of rows per INSERT statement. Default: 500.
                          Postgres supports up to ~65k params; SQLite max is 999
                          total params, so keep batch_size low for wide tables.
        validate:         If True, runs ``full_clean()`` on each instance before
                          inserting. Slows things down but catches bad data early.
        ignore_conflicts: If True, add ``ON CONFLICT DO NOTHING`` (Postgres) or
                          ``INSERT IGNORE`` (MySQL). No-op on SQLite (uses OR IGNORE).

    Returns:
        The same list of instances (pks may not be set — depends on the DB
        driver's ability to return them from a multi-row INSERT).

    Signals:
        Does NOT fire pre_save / post_save to keep bulk operations fast.
        Connect to ``pre_bulk_create`` / ``post_bulk_create`` if needed.
    """
    from ryx.models import _apply_auto_timestamps

    if not instances:
        return list(instances)

    # Validate if requested
    if validate:
        for inst in instances:
            await inst.full_clean()

    # Apply auto timestamps
    for inst in instances:
        _apply_auto_timestamps(inst, created=True)

    # Determine which fields to insert (non-pk, editable + auto_now_add)
    fields = [
        f
        for f in model._meta.fields.values()
        if not f.primary_key and (f.editable or getattr(f, "auto_now_add", False))
    ]
    col_names = [f.column for f in fields]

    if not col_names:
        return list(instances)

    pk_field = model._meta.pk_field

    # Process in batches
    for batch in _chunked(instances, batch_size):
        pks = await _insert_batch(model, batch, fields, col_names, ignore_conflicts)
        # Assign returned PKs to instances
        for inst, pk in zip(batch, pks):
            object.__setattr__(inst, pk_field.attname, pk)

    return list(instances)


async def _insert_batch(
    model: Type["Model"],
    batch: Sequence["Model"],
    fields: list,
    col_names: list,
    ignore_conflicts: bool,
) -> list:
    """Execute a single multi-row INSERT for one batch.

    Returns the list of assigned PKs (from RETURNING clause).
    """
    from ryx.pool_ext import fetch_with_params

    # Build quoted column list
    quoted_cols = ", ".join(f'"{c}"' for c in col_names)

    # Collect all values and build placeholder rows
    all_values = []
    row_placeholders = []
    for inst in batch:
        row_vals = [f.to_db(getattr(inst, f.attname)) for f in fields]
        all_values.extend(row_vals)
        row_placeholders.append(f"({', '.join('?' for _ in fields)})")

    values_sql = ", ".join(row_placeholders)

    # Backend-aware conflict handling
    backend = _detect_backend()
    if ignore_conflicts:
        if backend == "postgres":
            # Postgres: ON CONFLICT DO NOTHING
            conflict_suffix = "ON CONFLICT DO NOTHING"
            insert_kw = "INSERT INTO"
        elif backend == "mysql":
            # MySQL: INSERT IGNORE
            conflict_suffix = ""
            insert_kw = "INSERT IGNORE INTO"
        else:
            # SQLite: INSERT OR IGNORE
            conflict_suffix = ""
            insert_kw = "INSERT OR IGNORE INTO"
    else:
        conflict_suffix = ""
        insert_kw = "INSERT INTO"

    pk_field = model._meta.pk_field
    pk_col = pk_field.column if pk_field else "id"

    # RETURNING is not supported with ON CONFLICT DO NOTHING on all backends,
    # and MySQL doesn't support RETURNING at all.
    if backend == "postgres" and conflict_suffix:
        # Postgres supports RETURNING with ON CONFLICT DO NOTHING
        sql = (
            f'{insert_kw} "{model._meta.table_name}" ({quoted_cols}) '
            f'VALUES {values_sql} {conflict_suffix} RETURNING "{pk_col}"'
        )
    elif backend == "mysql":
        # MySQL: no RETURNING support
        sql = (
            f'{insert_kw} "{model._meta.table_name}" ({quoted_cols}) '
            f"VALUES {values_sql}"
        )
    else:
        # SQLite: RETURNING works without conflict clause
        sql = (
            f'{insert_kw} "{model._meta.table_name}" ({quoted_cols}) '
            f'VALUES {values_sql} {conflict_suffix} RETURNING "{pk_col}"'
        )

    # Fetch returned IDs
    if backend == "mysql":
        # MySQL doesn't support RETURNING — execute and return empty list
        from ryx.pool_ext import execute_with_params

        await execute_with_params(sql, all_values)
        return []

    rows = await fetch_with_params(sql, all_values)
    return [row[pk_col] for row in rows if pk_col in row]


####    bulk_update
async def bulk_update(
    model: Type["Model"],
    instances: Sequence["Model"],
    fields: List[str],
    *,
    batch_size: int = 500,
) -> int:
    """Update specific fields on many instances using CASE WHEN.

    Generates a single UPDATE statement per batch with CASE WHEN clauses::

        UPDATE "table" SET
            "col1" = CASE "pk" WHEN 1 THEN ? WHEN 2 THEN ? END,
            "col2" = CASE "pk" WHEN 1 THEN ? WHEN 2 THEN ? END
        WHERE "pk" IN (?, ?, ...)

    This is dramatically faster than N individual UPDATE statements because
    it requires only one DB round-trip per batch instead of N.

    Args:
        model:      The Model class.
        instances:  Model instances with updated field values.
        fields:     Field names to update (must not include pk).
        batch_size: Max instances per UPDATE statement.  Default: 500.

    Returns:
        Total number of rows updated.

    Signals:
        Does NOT fire pre_save / post_save signals (for performance).
    """
    if not instances or not fields:
        return 0

    pk_field = model._meta.pk_field
    if not pk_field:
        raise ValueError(f"{model.__name__} has no primary key")

    # Filter out pk from fields
    update_fields = [f for f in fields if f != pk_field.attname]
    if not update_fields:
        return 0

    field_objs = {
        name: model._meta.fields[name]
        for name in update_fields
        if name in model._meta.fields
    }
    total = 0

    from ryx import ryx_core as _core
    from ryx.pool_ext import execute_with_params

    for batch in _chunked(instances, batch_size):
        # Collect valid instances (with pk set)
        valid = [inst for inst in batch if inst.pk is not None]
        if not valid:
            continue

        pks = [inst.pk for inst in valid]
        pk_col = pk_field.column
        table = model._meta.table_name

        # Build CASE WHEN clauses.
        # Strategy: inline integers directly in SQL (zero FFI cost),
        # use ? placeholders only for non-integer values.
        case_clauses = []
        all_values = []

        for fname in update_fields:
            if fname not in field_objs:
                continue
            fobj = field_objs[fname]
            col = fobj.column
            case_parts = [f'"{col}" = CASE "{pk_col}"']
            for inst in valid:
                val = fobj.to_db(getattr(inst, fname))
                if isinstance(val, int) and not isinstance(val, bool):
                    # Inline integers — zero FFI overhead
                    case_parts.append(f"WHEN {inst.pk} THEN {val}")
                else:
                    case_parts.append("WHEN ? THEN ?")
                    all_values.append(inst.pk)
                    all_values.append(val)
            case_parts.append("END")
            case_clauses.append(" ".join(case_parts))

        if not case_clauses:
            continue

        # WHERE IN — inline integer PKs
        pk_parts = []
        for pk in pks:
            if isinstance(pk, int):
                pk_parts.append(str(pk))
            else:
                pk_parts.append("?")
                all_values.append(pk)

        sql = (
            f'UPDATE "{table}" SET '
            f"{', '.join(case_clauses)} "
            f'WHERE "{pk_col}" IN ({", ".join(pk_parts)})'
        )

        if all_values:
            await execute_with_params(sql, all_values)
        else:
            from ryx.executor_helpers import raw_execute

            await raw_execute(sql)
        total += len(valid)

    return total


####    bulk_delete
async def bulk_delete(
    model: Type["Model"],
    instances: Sequence["Model"],
    *,
    batch_size: int = 500,
) -> int:
    """Delete many model instances in batched DELETE ... WHERE pk IN (...) queries.

    Batching is required because SQLite has a hard limit of 999 bound
    parameters per statement.  With a default ``batch_size`` of 500, a
    single-row table (just the PK) can safely delete up to 500 rows per
    statement.

    Args:
        model:      The Model class.
        instances:  Instances to delete (must have pks set).
        batch_size: Max instances per DELETE statement.  Default: 500.

    Returns:
        Total number of rows deleted.

    Signals:
        Does NOT fire pre_delete / post_delete signals.
    """
    pk_field = model._meta.pk_field
    if not pk_field:
        raise ValueError(f"{model.__name__} has no primary key")

    pks = [inst.pk for inst in instances if inst.pk is not None]
    if not pks:
        return 0

    from ryx import ryx_core as _core

    total = 0
    for batch in _chunked(pks, batch_size):
        total += await _core.bulk_delete(
            model._meta.table_name, pk_field.column, list(batch)
        )
    return total


#
# Streaming (async generator)
#
async def stream(
    queryset,
    *,
    chunk_size: int = 100,
) -> None:
    """Async generator that yields model instances in chunks.

    Keeps memory usage bounded by fetching ``chunk_size`` rows at a time
    using LIMIT/OFFSET pagination.

    Usage::

        async for post in stream(Post.objects.filter(active=True), chunk_size=50):
            process(post)

    Args:
        queryset:   Any QuerySet instance.
        chunk_size: Number of rows per DB fetch. Default: 100.

    Yields:
        Model instances one at a time.

    Note:
        This uses LIMIT/OFFSET pagination internally. For very large tables
        (millions of rows), consider keyset pagination instead:
        ``Post.objects.filter(id__gt=last_seen_id).order_by("id").limit(100)``
    """
    offset = 0
    while True:
        batch_qs = queryset.limit(chunk_size).offset(offset)
        batch = await batch_qs
        if not batch:
            break
        for instance in batch:
            yield instance
        if len(batch) < chunk_size:
            break
        offset += chunk_size


####    Internal helpers
def _chunked(iterable: Sequence, n: int):
    """Yield successive n-sized chunks from iterable."""
    it = list(iterable)
    for i in range(0, len(it), n):
        yield it[i : i + n]
