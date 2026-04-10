"""
ryx_core — type stubs for the compiled Rust extension
========================================================

This file is the **only** stub file for Ryx's Rust layer.  It covers every
symbol that ``src/lib.rs`` exposes to Python via PyO3:

* Two classes:   ``QueryBuilder``, ``TransactionHandle``
* Ten functions: ``setup``, ``register_lookup``, ``available_lookups``,
                 ``is_connected``, ``pool_stats``, ``raw_fetch``,
                 ``raw_execute``, ``execute_with_params``,
                 ``fetch_with_params``, ``begin_transaction``
* One constant:  ``__version__``

Typing conventions
------------------
* Rust ``async`` functions (``future_into_py``) are typed as ``async def``
  so ``await core.fetch_all()`` type-checks correctly.
* Row data is ``dict[str, Any]``: sqlx decodes columns to the best-fit
  Python primitive but the schema is only known at runtime.
* ``value: object`` on ``add_filter`` / ``execute_insert`` / ``execute_update``
  accepts any Python scalar that ``py_to_sql_value`` handles: ``None``,
  ``bool``, ``int``, ``float``, ``str``, ``list``, ``tuple``.
* Rust ``u64`` / ``u32`` become ``int``; Rust ``bool`` stays ``bool``.
"""

from __future__ import annotations

from typing import Any, Optional

# ---------------------------------------------------------------------------
# Module constant
# ---------------------------------------------------------------------------

__version__: str
"""Semver version of the compiled Rust core, e.g. ``"0.2.0"``."""

# 
# Module-level functions
# 
async def setup(
    urls: dict, 
    max_connections: int = 10,
    min_connections: int = 1,
    connect_timeout: int = 30,
    idle_timeout: int = 600,
    max_lifetime: int = 1800,
) -> None:
    """Initialise the global sqlx connection pool.

    Must be called **once** before any query.  Call it in your application
    startup (e.g. FastAPI ``lifespan``, ASGI ``startup`` event, or top of a
    script).

    Parameters
    ----------
    url:
        Connection URL.  Supported schemes:

        - ``postgres://user:pass@host:5432/db``
        - ``mysql://user:pass@host:3306/db``
        - ``sqlite:///absolute/path.db``
        - ``sqlite::memory:``

    max_connections:
        Pool size upper bound.  Default ``10``.
    min_connections:
        Minimum idle connections to keep alive.  Default ``1``.
    connect_timeout:
        Seconds before a connection attempt is abandoned.  Default ``30``.
    idle_timeout:
        Seconds of inactivity before a connection is closed.  Default ``600``.
    max_lifetime:
        Maximum age of any single connection in seconds.  Default ``1800``.

    Raises
    ------
    RuntimeError
        If the pool is already initialised, or the database is unreachable.
    """
    ...


def register_lookup(name: str, sql_template: str) -> None:
    """Register a custom filter lookup operator (process-global, thread-safe).

    After registration the lookup is immediately usable in
    ``QueryBuilder.add_filter`` and in
    ``ryx.queryset.QuerySet.filter`` / ``.exclude``.

    Parameters
    ----------
    name:
        The lookup suffix, e.g. ``"ilike"`` to enable
        ``filter(title__ilike="%python%")``.
    sql_template:
        A SQL fragment containing exactly two placeholders:

        - ``{col}`` — replaced with the double-quoted column reference.
        - ``?``     — replaced with the bound parameter.

        Examples::

            "{col} ILIKE ?"
            "to_tsvector('english', {col}) @@ plainto_tsquery(?)"
            "{col} % 2 = 0"

    Raises
    ------
    RuntimeError
        If the internal lookup registry is not initialised (should never
        happen after a normal ``import ryx``).
    """
    ...


def available_lookups() -> list[str]:
    """Return the names of all registered lookups, sorted alphabetically.

    Includes both built-in lookups and any added by ``register_lookup``.

    Built-in lookups
    ----------------
    ``contains``, ``endswith``, ``exact``, ``gt``, ``gte``, ``icontains``,
    ``iendswith``, ``in``, ``isnull``, ``istartswith``, ``lt``, ``lte``,
    ``range``, ``startswith``
    """
    ...


def list_lookups() -> list[str]:
    """Return all registered lookup names (built-in + custom)."""
    ...


def list_transforms() -> list[str]:
    """Return all registered transform names (built-in + custom)."""
    ...

def list_aliases() -> list[str]:
    """Return all configured databases aliases"""
    ...


def is_connected(alias: str = 'default') -> bool:
    """Return ``True`` if ``setup()`` has been called successfully.

    Pure in-memory check — no database round-trip.
    """
    ...


def pool_stats() -> dict[str, int]:
    """Return live statistics for the connection pool.

    Returns
    -------
    dict with two keys:

    ``"size"``
        Total open connections (active + idle).
    ``"idle"``
        Connections currently waiting for a query.

    Raises
    ------
    RuntimeError
        If ``setup()`` has not been called.
    """
    ...


async def raw_fetch(sql: str) -> list[dict[str, Any]]:
    """Execute a parameter-free ``SELECT`` string and return all rows.

    **Internal — migration runner only.**  Bypasses the QueryBuilder and
    all safety checks.  Do not pass user-supplied data in ``sql``.

    Parameters
    ----------
    sql:
        A complete, self-contained SQL ``SELECT`` statement with no ``?``
        placeholders.

    Returns
    -------
    List of row dicts.  Each dict maps ``column_name → Python value``
    (``int``, ``float``, ``str``, ``bool``, or ``None``).

    Raises
    ------
    RuntimeError
        If ``setup()`` has not been called, or on SQL / driver errors.
    """
    ...


async def raw_execute(sql: str) -> None:
    """Execute a parameter-free DDL / DML string.

    **Internal — migration runner only.**  Used for ``CREATE TABLE``,
    ``ALTER TABLE``, ``CREATE INDEX``, ``DROP TABLE``, etc.

    Parameters
    ----------
    sql:
        A complete SQL string with no ``?`` placeholders.

    Raises
    ------
    RuntimeError
        If ``setup()`` has not been called, or on SQL / driver errors.
    """
    ...


async def execute_with_params(sql: str, values: list[object]) -> int:
    """Execute a parameterized statement and return the rows-affected count.

    **Internal — ``ryx.bulk.bulk_create`` only.**  Handles multi-row
    ``INSERT`` statements whose structure cannot be expressed through the
    ``QueryBuilder`` AST.

    Parameters
    ----------
    sql:
        SQL string with one ``?`` per bind value, in order.
    values:
        Python values to bind.  Each element may be ``None``, ``bool``,
        ``int``, ``float``, ``str``, a ``list``, or a ``tuple``.

    Returns
    -------
    Number of rows affected (``int``).

    Raises
    ------
    RuntimeError
        If ``setup()`` has not been called, or on SQL / type errors.
    """
    ...


async def fetch_with_params(sql: str, values: list[object]) -> list[dict[str, Any]]:
    """Execute a parameterized ``SELECT`` and return rows.

    **Internal — ``ryx.descriptors.ManyToManyManager`` only.**

    Parameters
    ----------
    sql:
        SQL ``SELECT`` string with ``?`` placeholders.
    values:
        Python values to bind (same type rules as ``execute_with_params``).

    Returns
    -------
    List of row dicts.

    Raises
    ------
    RuntimeError
        If ``setup()`` has not been called, or on SQL errors.
    """
    ...


async def begin_transaction() -> TransactionHandle:
    """Acquire a connection and begin a new database transaction.

    Called by ``ryx.transaction.TransactionContext.__aenter__``.
    Prefer the high-level context manager over calling this directly::

        async with ryx.transaction() as tx:
            ...

    Returns
    -------
    A live ``TransactionHandle``.

    Raises
    ------
    RuntimeError
        If ``setup()`` has not been called, or pool exhaustion occurs.
    """
    ...


def _set_active_transaction(tx: 'TransactionHandle' | None) -> None:
    """Internal API: track the active transaction for QueryBuilder execution."""
    ...


# ---------------------------------------------------------------------------
# QueryBuilder
# ---------------------------------------------------------------------------

class QueryBuilder:
    """Immutable SQL query builder backed by the Rust ``QueryNode`` AST.

    Every mutating method returns a **new** ``QueryBuilder`` — the original
    is never modified.  This is the same persistent / value-object pattern
    used by sqlx's own query builder internally.

    ``QueryBuilder`` is the private engine inside
    ``ryx.queryset.QuerySet``.  Most application code should use the
    high-level ``QuerySet`` API rather than constructing a ``QueryBuilder``
    directly.

    Quick reference
    ---------------
    Builder methods (return a new ``QueryBuilder``):

    +---------------------------+------------------------------------------+
    | Method                    | SQL effect                               |
    +===========================+==========================================+
    | ``add_filter(...)``       | ``WHERE col lookup ?``                   |
    | ``add_q_node(...)``       | ``WHERE (… OR …)`` / Q-tree              |
    | ``add_annotation(...)``   | ``SELECT agg(col) AS alias``             |
    | ``add_group_by(field)``   | ``GROUP BY col``                         |
    | ``add_join(...)``         | ``[INNER|LEFT|…] JOIN …``                |
    | ``add_order_by(field)``   | ``ORDER BY col [DESC]``                  |
    | ``set_limit(n)``          | ``LIMIT n``                              |
    | ``set_offset(n)``         | ``OFFSET n``                             |
    | ``set_distinct()``        | ``SELECT DISTINCT …``                    |
    +---------------------------+------------------------------------------+

    Execution methods (``async``, return data or row counts):

    +---------------------------+------------------------------------------+
    | Method                    | SQL / return type                        |
    +===========================+==========================================+
    | ``fetch_all()``           | ``SELECT …``  → ``list[dict]``           |
    | ``fetch_first()``         | ``SELECT … LIMIT 1`` → ``dict | None``  |
    | ``fetch_get()``           | asserts exactly 1 row → ``dict``         |
    | ``fetch_count()``         | ``SELECT COUNT(*)`` → ``int``            |
    | ``fetch_aggregate()``     | aggregate-only SELECT → ``dict``         |
    | ``execute_delete()``      | ``DELETE FROM … WHERE …`` → ``int``      |
    | ``execute_update(...)``   | ``UPDATE … SET … WHERE …`` → ``int``     |
    | ``execute_insert(...)``   | ``INSERT INTO …`` → ``int`` (pk or count)|
    +---------------------------+------------------------------------------+

    Introspection:

    ``compiled_sql()`` — returns the SQL string (no execution, ``?``
    placeholders not filled in).
    """

    def __init__(self, table: str) -> None:
        """Create a ``SELECT *`` query against *table*.

        Parameters
        ----------
        table:
            Unquoted table name.  The Rust compiler will double-quote it,
            e.g. ``"posts"`` → ``"posts"`` in the emitted SQL.
        """
        ...

    # Filter / WHERE
    def add_filter(
        self,
        field: str,
        lookup: str,
        value: object,
        negated: bool = False,
    ) -> "QueryBuilder":
        """Append a WHERE condition.  Multiple calls are AND-ed.

        Parameters
        ----------
        field:
            Column reference.  Unqualified (``"views"``) or
            table-qualified (``"posts.author_id"``).
        lookup:
            A lookup name from ``available_lookups()``.  Common values:
            ``"exact"``, ``"gt"``, ``"gte"``, ``"lt"``, ``"lte"``,
            ``"contains"``, ``"icontains"``, ``"startswith"``,
            ``"istartswith"``, ``"endswith"``, ``"iendswith"``,
            ``"isnull"``, ``"in"``, ``"range"``.
        value:
            Bind value.  Accepted Python types: ``None``, ``bool``,
            ``int``, ``float``, ``str``, ``list[scalar]``, ``tuple[scalar]``.

            Special handling by the Rust compiler:

            - ``"isnull"`` — *value* is cast to bool; no bind param emitted.
            - ``"in"``     — *value* must be a list; expanded to ``IN (?,?,…)``.
              An empty list produces ``(1 = 0)`` (always false).
            - ``"range"``  — *value* must be ``[lo, hi]``; emits ``BETWEEN ? AND ?``.
            - ``"contains"`` / ``"icontains"`` etc. — ``%`` wrapping applied
              automatically to the string value.

        negated:
            If ``True``, wraps the condition in ``NOT (…)``.  This is what
            ``QuerySet.exclude()`` uses.

        Returns
        -------
        A new ``QueryBuilder`` with the condition appended.

        Raises
        ------
        ValueError
            If *lookup* is not in ``available_lookups()``.
        """
        ...

    def add_q_node(self, node: dict[str, Any]) -> "QueryBuilder":
        """Merge a Q-tree into the WHERE clause (AND with existing filters).

        Called by ``QuerySet.filter()`` when :class:`Q` objects with
        ``|`` (OR) or ``~`` (NOT) logic are passed.

        The *node* dict format (produced by ``Q.to_q_node()``):

        .. code-block:: python

            # Leaf node
            {
                "type":    "leaf",
                "field":   str,
                "lookup":  str,
                "value":   Any,
                "negated": bool,
            }

            # Combinator node
            {
                "type":     "and" | "or" | "not",
                "children": [<node>, ...],   # "not" has exactly one child
            }

        Parameters
        ----------
        node:
            Nested dict representing the Q-tree root.

        Returns
        -------
        A new ``QueryBuilder`` with the Q condition merged.

        Raises
        ------
        ValueError
            If *node* is missing required keys or has an unknown ``"type"``.
        """
        ...

    # Aggregation / GROUP BY
    def add_annotation(
        self,
        alias: str,
        func: str,
        field: str,
        distinct: bool = False,
    ) -> "QueryBuilder":
        """Add an aggregate expression to the SELECT list.

        Parameters
        ----------
        alias:
            Name used as the key in returned row dicts, e.g. ``"total_views"``.
        func:
            Aggregate function.  Recognised names: ``"Count"``, ``"Sum"``,
            ``"Avg"``, ``"Min"``, ``"Max"``.  Any other string is emitted
            verbatim as a raw SQL expression (for custom aggregates).
        field:
            Column to aggregate.  Use ``"*"`` with ``func="Count"`` to
            produce ``COUNT(*)``.
        distinct:
            If ``True``, inserts ``DISTINCT`` inside the aggregate call,
            e.g. ``COUNT(DISTINCT "user_id")``.

        Returns
        -------
        A new ``QueryBuilder`` with the annotation appended.
        """
        ...

    def add_group_by(self, field: str) -> "QueryBuilder":
        """Append a column to the ``GROUP BY`` clause.

        Parameters
        ----------
        field:
            Unquoted column name, e.g. ``"author_id"``.

        Returns
        -------
        A new ``QueryBuilder`` with the GROUP BY clause extended.
        """
        ...

    # JOIN
    def add_join(
        self,
        kind: str,
        table: str,
        alias: str,
        on_left: str,
        on_right: str,
    ) -> "QueryBuilder":
        """Append a JOIN clause.

        Parameters
        ----------
        kind:
            Join type string (case-insensitive).  Accepted values:
            ``"INNER"``, ``"LEFT"``, ``"LEFT OUTER"``, ``"RIGHT"``,
            ``"RIGHT OUTER"``, ``"FULL"``, ``"FULL OUTER"``, ``"CROSS"``.
            Anything else is treated as ``INNER JOIN``.
        table:
            Name of the table to join (unquoted).
        alias:
            SQL alias for the joined table, e.g. ``"a"``.
            Pass an empty string ``""`` for no alias.
        on_left:
            Left side of the ``ON`` condition.  May be table-qualified:
            ``"posts.author_id"``.
        on_right:
            Right side of the ``ON`` condition, e.g. ``"a.id"``.

        Returns
        -------
        A new ``QueryBuilder`` with the JOIN clause appended.

        Note
        ----
        For ``CROSS JOIN`` the ``on_left`` / ``on_right`` values are
        ignored (no ``ON`` clause is emitted).
        """
        ...

    # Ordering / pagination
    def add_order_by(self, field: str) -> "QueryBuilder":
        """Append an ``ORDER BY`` term.

        Parameters
        ----------
        field:
            Unquoted column name.  Prefix with ``"-"`` for descending:
            ``"-views"`` → ``ORDER BY "views" DESC``.
            Without prefix: ``"title"`` → ``ORDER BY "title" ASC``.

        Returns
        -------
        A new ``QueryBuilder`` with the ordering appended.  Multiple calls
        accumulate; earlier calls take higher sort priority.
        """
        ...

    def set_limit(self, n: int) -> "QueryBuilder":
        """Set the ``LIMIT`` clause.

        Parameters
        ----------
        n:
            Maximum number of rows to return.

        Returns
        -------
        A new ``QueryBuilder`` with the limit set, overwriting any
        previous limit.
        """
        ...

    def set_offset(self, n: int) -> "QueryBuilder":
        """Set the ``OFFSET`` clause.

        Parameters
        ----------
        n:
            Number of leading result rows to skip.

        Returns
        -------
        A new ``QueryBuilder`` with the offset set.
        """
        ...

    def set_distinct(self) -> "QueryBuilder":
        """Enable ``SELECT DISTINCT``.

        Returns
        -------
        A new ``QueryBuilder`` with DISTINCT turned on.
        """
        ...

    def set_using(alias: str) -> "QueryBuilder":
        """Set the database to use for this query
        
        Returns
        -------
        A new ``QueryBuilder`` with bd_alias set to the new alias.
        """
        ...

    # Introspection
    def compiled_sql(self) -> str:
        """Return the compiled SQL string without executing the query.

        Bind values are **not** interpolated — ``?`` placeholders remain
        in the output.  Useful for logging and debugging.

        Example output::

            'SELECT * FROM "posts" WHERE "active" = ? ORDER BY "views" DESC LIMIT 10'

        Returns
        -------
        Complete SQL string.

        Raises
        ------
        ValueError
            If any filter references an unregistered lookup name.
        """
        ...

    # Async execution
    async def fetch_all(self) -> list[dict[str, Any]]:
        """Execute the current SELECT and return all matching rows.

        Returns
        -------
        A list of row dicts.  Each dict maps ``column_name → value``
        where *value* is the most appropriate Python type decoded by the
        sqlx driver: ``int``, ``float``, ``str``, ``bool``, or ``None``.
        Returns ``[]`` when no rows match.

        Raises
        ------
        RuntimeError
            If ``setup()`` has not been called, or on SQL / driver errors.
        """
        ...

    async def fetch_first(self) -> Optional[dict[str, Any]]:
        """Execute ``SELECT … LIMIT 1`` and return the first row.

        Internally calls ``set_limit(1)`` then ``fetch_all``.

        Returns
        -------
        A single row dict, or ``None`` when no rows match.

        Raises
        ------
        RuntimeError
            If ``setup()`` has not been called.
        """
        ...

    async def fetch_get(self) -> dict[str, Any]:
        """Execute the SELECT and assert exactly one row is returned.

        This is the Rust engine behind ``QuerySet.get()``.

        Returns
        -------
        A single row dict.

        Raises
        ------
        RuntimeError
            ``"No matching object found"`` — zero rows matched the filters.
        RuntimeError
            ``"multiple"`` — more than one row matched.
        RuntimeError
            If ``setup()`` has not been called.
        """
        ...

    async def fetch_count(self) -> int:
        """Execute ``SELECT COUNT(*)`` and return the integer result.

        The count respects all active filters (``add_filter``,
        ``add_q_node``) and JOINs but ignores ``LIMIT``, ``OFFSET``, and
        ``ORDER BY``.

        Returns
        -------
        ``int`` — number of matching rows.

        Raises
        ------
        RuntimeError
            If ``setup()`` has not been called.
        """
        ...

    async def fetch_aggregate(self) -> dict[str, Any]:
        """Execute an aggregate-only SELECT and return a single result dict.

        Switches the builder's internal operation to ``Aggregate`` mode,
        which emits ``SELECT agg1(...) AS alias1, agg2(...) AS alias2 …``
        with no row-level columns.

        This is the Rust engine behind ``QuerySet.aggregate()``.

        Returns
        -------
        Dict mapping each annotation *alias* → computed scalar value.
        Returns ``{}`` if no rows matched (aggregate over empty set).

        Raises
        ------
        RuntimeError
            If ``setup()`` has not been called, or if no annotations have
            been added via ``add_annotation``.
        """
        ...

    async def execute_delete(self) -> int:
        """Execute ``DELETE FROM … WHERE …`` and return rows deleted.

        The WHERE clause is built from all active ``add_filter`` /
        ``add_q_node`` calls.  A builder with **no** filters deletes
        **all** rows — use with care.

        Returns
        -------
        Number of deleted rows (``int``), or ``0`` when no rows matched.

        Raises
        ------
        RuntimeError
            If ``setup()`` has not been called.
        """
        ...

    async def execute_update(
        self,
        assignments: list[tuple[str, object]],
    ) -> int:
        """Execute ``UPDATE … SET … WHERE …`` and return rows updated.

        Parameters
        ----------
        assignments:
            List of ``(column_name, new_value)`` pairs.  Column names are
            unquoted; they will be double-quoted by the compiler.  Values
            follow the same type rules as ``add_filter``'s *value*
            parameter.

        Returns
        -------
        Number of updated rows (``int``), or ``0`` when no rows matched.

        Raises
        ------
        RuntimeError
            If ``setup()`` has not been called.
        RuntimeError
            If *assignments* is empty (``UPDATE`` with no ``SET`` is invalid).
        """
        ...

    async def execute_insert(
        self,
        values: list[tuple[str, object]],
        returning_id: bool = False,
    ) -> int:
        """Execute ``INSERT INTO … (…) VALUES (…)`` and return the result.

        Parameters
        ----------
        values:
            List of ``(column_name, value)`` pairs for the new row.
            Column names are unquoted; values follow the same type rules
            as ``add_filter``'s *value* parameter.
        returning_id:
            When ``True``, appends ``RETURNING id`` to the SQL
            (Postgres-native; sqlx parses the returned id from the
            driver result set).  On databases that don't support
            ``RETURNING`` the rows-affected count is returned instead.

        Returns
        -------
        The newly-created primary key (``int``) when *returning_id* is
        ``True`` and the driver supports it; otherwise the rows-affected
        count (always ``1`` for a successful single-row insert).

        Raises
        ------
        RuntimeError
            If ``setup()`` has not been called.
        RuntimeError
            If *values* is empty.
        """
        ...


#
# TransactionHandle
#
class TransactionHandle:
    """A live database transaction, owned by the Rust ``Arc<Mutex<Option<…>>>``.

    Obtained by awaiting ``begin_transaction()``.  Application code should
    use the higher-level context manager instead of instantiating or calling
    this class directly::

        async with ryx.transaction() as tx:
            await Post.objects.create(title="Atomic write")
            await tx.savepoint("before_risky_part")
            try:
                await do_risky_thing()
            except SomeError:
                await tx.rollback_to("before_risky_part")
                raise

    All methods are **coroutines** — they must be awaited.

    After ``commit()`` or ``rollback()`` the handle is *exhausted*:
    further calls are safe no-ops (the inner ``Option`` becomes ``None``).
    """

    async def commit(self) -> None:
        """Commit the transaction.

        Flushes all pending changes to the database and releases the
        connection back to the pool.

        Idempotent: safe to call on an already-committed or
        already-rolled-back handle.

        Raises
        ------
        RuntimeError
            On rare database-level commit failures (e.g. network drop
            after the command was sent).
        """
        ...

    async def rollback(self) -> None:
        """Roll back the transaction.

        Discards all changes made since the transaction began (or since
        the last savepoint that was itself committed) and releases the
        connection.

        Idempotent: safe to call multiple times.

        Raises
        ------
        RuntimeError
            On rare database-level rollback failures.
        """
        ...

    async def savepoint(self, name: str) -> None:
        """Create a named ``SAVEPOINT`` within the current transaction.

        Savepoints enable partial rollback: calling ``rollback_to(name)``
        reverts only the changes made *after* this savepoint, leaving
        earlier changes intact and the transaction open.

        Parameters
        ----------
        name:
            A valid SQL identifier used to refer to this savepoint,
            e.g. ``"before_items"`` or ``"sp_1"``.  Must be unique within
            the transaction.

        Raises
        ------
        RuntimeError
            If the transaction has already been committed or rolled back.
        RuntimeError
            If the database rejects the savepoint name (e.g. duplicate).
        """
        ...

    async def rollback_to(self, name: str) -> None:
        """Roll back to a previously created savepoint.

        Undoes all database changes made *after* the savepoint was created.
        The transaction remains open; further queries can be executed.

        Parameters
        ----------
        name:
            The savepoint name that was passed to ``savepoint()``.

        Raises
        ------
        RuntimeError
            If the transaction is no longer active (already committed /
            rolled back).
        RuntimeError
            If no savepoint named *name* exists in the current transaction.
        """
        ...

    async def is_active(self) -> bool:
        """Return whether the transaction is still live.

        Returns
        -------
        ``True``  — ``commit()`` / ``rollback()`` have not been called yet.
        ``False`` — the transaction has ended.
        """
        ...
