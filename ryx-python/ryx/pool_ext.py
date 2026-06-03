"""
Ryx ORM — Pool Extension Helpers

Provides Python-accessible wrappers for parameterized raw SQL execution
that goes through the Rust pool. Used by bulk_create and other operations
that need to bind parameters but bypass the QueryBuilder AST.

These are internal helpers — not part of the public API.
"""

from __future__ import annotations

import logging
from typing import Any, List

from ryx import ryx_core as _core

logger = logging.getLogger("ryx.pool")


async def execute_with_params(sql: str, values: List[Any]) -> int:
    """Execute a parameterized SQL statement and return rows_affected.

    Args:
        sql:    SQL string with ``?`` placeholders.
        values: Flat list of bind values matching placeholder positions.

    Returns:
        Number of rows affected.
    """
    logger.debug("EXEC: %s [%d params]", sql[:120], len(values))
    return await _core.execute_with_params(sql, values)


async def fetch_with_params(sql: str, values: List[Any]) -> list:
    """Execute a parameterized SELECT and return rows as list of dicts.

    Args:
        sql:    SQL SELECT string with ``?`` placeholders.
        values: Flat list of bind values.

    Returns:
        List of row dicts.
    """
    logger.debug("FETCH: %s [%d params]", sql[:120], len(values))
    return await _core.fetch_with_params(sql, values)
