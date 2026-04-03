"""
Ryx ORM — Raw SQL Executor Helpers

These functions provide a low-level escape hatch for executing raw SQL
directly through the Rust executor, bypassing the QueryBuilder and AST.

They are intentionally NOT part of the public API. They exist to serve:
  1. The migration runner (DDL and information_schema queries)
  2. Internal diagnostic tools

Never expose these to end users — they accept raw SQL strings with no
escaping or injection protection. The migration runner is the only
consumer and it constructs SQL from trusted (non-user-supplied) strings.

How it works:
  We create a minimal QueryBuilder targeting a dummy table, then call
  its execute methods with raw SQL via a special bypass path in Rust.

TODO: Expose a dedicated `raw_query()` function on the Rust side that
  accepts a complete SQL string + bound values, bypassing the AST entirely.
  For now, we directly instantiate the QueryBuilder and use `compiled_sql`
  as a pass-through.
"""

from __future__ import annotations

from ryx import ryx_core as _core


async def raw_fetch(sql: str) -> list:
    """Execute a raw SELECT SQL string and return rows as a list of dicts.

    This is a low-level escape hatch. Use QuerySet for all application queries.

    Args:
        sql: A complete SQL SELECT string. Must NOT contain user input.

    Returns:
        A list of row dicts, same format as QuerySet results.
    """
    # We use a RawQueryBuilder to send the SQL directly to the executor.
    # This Rust function is registered in lib.rs specifically for this use case.
    return await _core.raw_fetch(sql)


async def raw_execute(sql: str) -> None:
    """Execute a raw DDL/DML SQL string with no return value.

    Args:
        sql: A complete SQL string (CREATE TABLE, ALTER TABLE, etc.).
             Must NOT contain user input.
    """
    await _core.raw_execute(sql)