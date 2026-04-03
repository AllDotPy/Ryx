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

    # Process in batches
    for batch in _chunked(instances, batch_size):
        await _insert_batch(model, batch, fields, col_names, ignore_conflicts)

    return list(instances)


async def _insert_batch(
    model: Type["Model"],
    batch: Sequence["Model"],
    fields: list,
    col_names: list,
    ignore_conflicts: bool,
) -> None:
    """Execute a single multi-row INSERT for one batch."""

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

    # Conflict handling prefix/suffix
    if ignore_conflicts:
        # We detect backend by checking the URL (rough heuristic)
        # For now use the most compatible syntax
        insert_kw = "INSERT OR IGNORE INTO"  # SQLite
    else:
        insert_kw = "INSERT INTO"

    sql = f'{insert_kw} "{model._meta.table_name}" ({quoted_cols}) VALUES {values_sql}'

    # Use the raw executor via a QueryBuilder-style approach
    # We build a CompiledQuery manually and push it through the executor
    await _execute_raw_with_params(sql, all_values)


async def _execute_raw_with_params(sql: str, values: list) -> None:
    """Execute a SQL string with positional parameters via the pool."""

    # Build a temporary QueryBuilder that executes raw SQL.
    # We abuse execute_insert with a specially crafted node — actually we
    # use the executor directly by calling raw_execute for param-less SQL
    # or a direct pool execute for parameterized SQL.
    #
    # Since raw_execute in executor_helpers only handles no-param SQL, and our
    # bulk INSERT has params, we use the QueryBuilder execute_update pathway
    # with a pre-built SQL string. The cleanest way is a direct pool query.
    #
    # We implement this by using a Python-side async bridge to the Rust pool.
    from ryx.pool_ext import execute_with_params

    await execute_with_params(sql, values)


####    bulk_update
async def bulk_update(
    model: Type["Model"],
    instances: Sequence["Model"],
    fields: List[str],
    *,
    batch_size: int = 500,
) -> int:
    """Update specific fields on many instances efficiently.

    Uses individual UPDATE statements per instance (one per batch row) in a
    single transaction for atomicity. A future version will use CASE WHEN
    bulk updates.

    Args:
        model:      The Model class.
        instances:  Model instances with updated field values.
        fields:     Field names to update (must not include pk).
        batch_size: Number of updates per transaction batch.

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

    from ryx.transaction import transaction

    for batch in _chunked(instances, batch_size):
        async with transaction():
            for inst in batch:
                if inst.pk is None:
                    continue
                from ryx import ryx_core as _core

                assignments = [
                    (field_objs[f].column, field_objs[f].to_db(getattr(inst, f)))
                    for f in update_fields
                    if f in field_objs
                ]
                if not assignments:
                    continue
                builder = _core.QueryBuilder(model._meta.table_name)
                builder = builder.add_filter(
                    pk_field.column, "exact", inst.pk, negated=False
                )
                await builder.execute_update(assignments)
                total += 1

    return total


####    bulk_delete
async def bulk_delete(
    model: Type["Model"],
    instances: Sequence["Model"],
) -> int:
    """Delete many model instances in a single DELETE ... WHERE pk IN (...).

    Args:
        model:     The Model class.
        instances: Instances to delete (must have pks set).

    Returns:
        Number of rows deleted.

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

    builder = _core.QueryBuilder(model._meta.table_name)
    # We pass pks as a list for the __in lookup
    builder = builder.add_filter(pk_field.column, "in", pks, negated=False)
    return await builder.execute_delete()


# ──────────────────────────────────────────────────────────────────────────────
# Streaming (async generator)
# ──────────────────────────────────────────────────────────────────────────────


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
