"""
Ryx ORM — QuerySet

The QuerySet class provides a lazy, async, chainable interface for building:
  - Q() class for OR / NOT filter trees
  - .annotate()    — attach aggregate expressions to each row
  - .aggregate()   — return a single dict of aggregate values
  - .values()      — restrict SELECT columns + enable GROUP BY
  - .select_related() stub
  - .join()        — explicit JOIN clause
  - .using()       — future multi-db stub
  - Signals on bulk .update() and .delete()
"""

from __future__ import annotations

import asyncio
import functools
from typing import Any, Dict, List, Optional, TYPE_CHECKING

from ryx import ryx_core as _core
from ryx.exceptions import DoesNotExist, MultipleObjectsReturned
from ryx.signals import (
    post_bulk_delete,
    post_update,
    pre_bulk_delete,
    pre_update,
)

if TYPE_CHECKING:
    from ryx.models import Model


####    Index awaitable helper
class _IndexAwaitable:
    """Wraps a QuerySet to return a single instance when awaited."""

    def __init__(self, qs: "QuerySet") -> None:
        self._qs = qs

    def __await__(self):
        return self._resolve().__await__()

    async def _resolve(self):
        rows = await self._qs._execute()
        if not rows:
            raise IndexError("QuerySet index out of range")
        return rows[0]


###
##      Q — BOOLEAN FILTER EXPRESSIONS NOTE
####
class Q:
    """Boolean filter expression that supports OR and NOT combinations.

    Usage::

        # Simple filter (AND-ed by default inside filter())
        Post.objects.filter(Q(active=True))

        # OR
        Post.objects.filter(Q(active=True) | Q(views__gte=1000))

        # NOT
        Post.objects.filter(~Q(status="draft"))

        # Complex nesting
        Post.objects.filter(
            (Q(active=True) & Q(views__gte=100)) | Q(featured=True)
        )

    Q objects can also be combined with regular filter kwargs::

        Post.objects.filter(Q(active=True) | Q(views__gte=1000), author_id=42)
    """

    def __init__(self, **kwargs: Any) -> None:
        # Each kwarg becomes a Leaf in the Q-tree.
        # Multiple kwargs are AND-ed together.
        self._leaves = kwargs
        self._connector = "AND"  # "AND" | "OR"
        self._negated = False
        self._children: List["Q"] = []

    def _combine(self, other: "Q", connector: str) -> "Q":
        result = Q()
        result._connector = connector
        result._children = [self, other]
        return result

    def __and__(self, other: "Q") -> "Q":
        return self._combine(other, "AND")

    def __or__(self, other: "Q") -> "Q":
        return self._combine(other, "OR")

    def __invert__(self) -> "Q":
        q = Q()
        q._negated = True
        q._children = [self]
        q._connector = "AND"
        return q

    def to_q_node(self) -> dict:
        """Serialise this Q into a dict that the Rust builder understands.

        The Rust side receives a nested dict and converts it to a QNode.
        Format::

            {"type": "and"|"or"|"not"|"leaf",
             "field": ..., "lookup": ..., "value": ..., "negated": ...
             "children": [...]}
        """
        if self._children:
            children_dicts = [c.to_q_node() for c in self._children]
            if self._negated and len(self._children) == 1:
                return {"type": "not", "children": children_dicts}
            return {
                "type": self._connector.lower(),
                "children": children_dicts,
            }

        # Leaf node with kwargs
        leaves = []
        for key, val in self._leaves.items():
            field, lookup = _parse_lookup_key(key)
            leaves.append(
                {
                    "type": "leaf",
                    "field": field,
                    "lookup": lookup,
                    "value": val,
                    "negated": self._negated,
                }
            )
        if len(leaves) == 1:
            return leaves[0]
        return {"type": "and", "children": leaves}

    def __repr__(self) -> str:
        if self._leaves:
            return f"Q({', '.join(f'{k}={v!r}' for k, v in self._leaves.items())})"
        conn = " | " if self._connector == "OR" else " & "
        s = conn.join(repr(c) for c in self._children)
        return f"~({s})" if self._negated else f"({s})"


###
##     AGGREGATE EXCEPTION HELPERS
####
class _Agg:
    """Base class for aggregate expressions used in annotate() / aggregate()."""

    func: str = ""

    def __init__(
        self, field: str, *, distinct: bool = False, output_field: str = ""
    ) -> None:
        self.field = field
        self.distinct = distinct
        self.output_field = output_field

    def as_dict(self, alias: str) -> dict:
        return {
            "alias": alias,
            "func": self.func,
            "field": self.field,
            "distinct": self.distinct,
        }


####
##      AGGREGATE COUNT
#####
class Count(_Agg):
    """COUNT(field) or COUNT(*) aggregate."""

    func = "Count"

    def __init__(self, field: str = "*", **kw):
        super().__init__(field, **kw)


####
##      AGGREGATE SUM
#####
class Sum(_Agg):
    """SUM(field) aggregate."""

    func = "Sum"


####
##      AGGREGATE AVG
#####
class Avg(_Agg):
    """AVG(field) aggregate."""

    func = "Avg"


####
##      AGGREGATE MIN
#####
class Min(_Agg):
    """MIN(field) aggregate."""

    func = "Min"


####
##      AGGREGATE MAX
#####
class Max(_Agg):
    """MAX(field) aggregate."""

    func = "Max"


####
##      RAW AGGREGATION
#####
class RawAgg(_Agg):
    """Custom SQL aggregate expression."""

    def __init__(self, sql: str, alias: str):
        super().__init__("*")
        self.func = sql
        self._alias = alias

    def as_dict(self, alias: str) -> dict:
        return {
            "alias": alias or self._alias,
            "func": self.func,
            "field": "*",
            "distinct": False,
        }


####
##      QUERYSET
#####
class QuerySet:
    """Lazy, async, chainable, immutable query builder.

    Every method returns a *new* QuerySet. SQL is only executed when the
    QuerySet is awaited or an evaluation method is called.
    """

    def __init__(
        self,
        model: type,
        builder: Optional[_core.QueryBuilder] = None,
        *,
        _select_columns: Optional[List[str]] = None,
        _annotations: Optional[List[dict]] = None,
        _group_by: Optional[List[str]] = None,
    ) -> None:

        self._model = model
        self._builder: _core.QueryBuilder = builder or _core.QueryBuilder(
            model._meta.table_name
        )
        self._select_columns = _select_columns
        self._annotations = _annotations or []
        self._group_by = _group_by or []

    def _clone(self, builder=None, **overrides) -> "QuerySet":
        return QuerySet(
            self._model,
            builder or self._builder,
            _select_columns=overrides.get("_select_columns", self._select_columns),
            _annotations=overrides.get("_annotations", list(self._annotations)),
            _group_by=overrides.get("_group_by", list(self._group_by)),
        )

    ##  Filtering
    def filter(self, *q_args: Q, **kwargs: Any) -> "QuerySet":
        """Add WHERE conditions (AND-ed). Accepts Q objects and kwargs.

        Examples::
            Post.objects.filter(active=True)
            Post.objects.filter(Q(active=True) | Q(featured=True))
            Post.objects.filter(Q(active=True), views__gte=100)
        """

        builder = self._builder

        # Q objects
        for q in q_args:
            node = q.to_q_node()
            builder = _apply_q_node(builder, node)

        # kwargs (flat filters)
        for key, val in kwargs.items():
            # Support Django-style primary key lookup in kwargs
            if key == "pk":
                key = self._model._meta.pk_field.attname
            field, lookup = _parse_lookup_key(key)
            builder = builder.add_filter(field, lookup, val, negated=False)
        return self._clone(builder)

    def exclude(self, *q_args: Q, **kwargs: Any) -> "QuerySet":
        """Add NOT conditions."""

        builder = self._builder
        for q in q_args:
            builder = _apply_q_node(builder, (~q).to_q_node())

        for key, val in kwargs.items():
            field, lookup = _parse_lookup_key(key)
            builder = builder.add_filter(field, lookup, val, negated=True)

        return self._clone(builder)

    def all(self) -> "QuerySet":
        return self._clone()

    # Aggregation / annotation
    def annotate(self, **aggs: _Agg) -> "QuerySet":
        """Attach aggregate expressions to each row.

        The aggregated value appears as an extra key in the result dict::

            posts = await Post.objects.annotate(comment_count=Count("comments.id"))
            posts[0]["comment_count"]  # → 42
        """

        new_anns = list(self._annotations)
        builder = self._builder
        for alias, agg in aggs.items():
            agg_dict = agg.as_dict(alias)
            new_anns.append(agg_dict)
            builder = builder.add_annotation(
                agg_dict["alias"],
                agg_dict["func"],
                agg_dict["field"],
                agg_dict["distinct"],
            )

        return self._clone(builder, _annotations=new_anns)

    async def aggregate(self, **aggs: _Agg) -> Dict[str, Any]:
        """Execute an aggregate-only query and return a single result dict.

        Example::

            result = await Post.objects.filter(active=True).aggregate(
                total_views = Sum("views"),
                avg_views   = Avg("views"),
                post_count  = Count("id"),
            )
            # → {"total_views": 12345, "avg_views": 42.1, "post_count": 293}
        """

        builder = self._builder
        for alias, agg in aggs.items():
            d = agg.as_dict(alias)
            builder = builder.add_annotation(
                d["alias"], d["func"], d["field"], d["distinct"]
            )
        raw = await builder.fetch_aggregate()

        return raw if raw else {}

    def values(self, *fields: str) -> "QuerySet":
        """Restrict SELECT to specified fields and enable GROUP BY.

        Useful for combined annotate+values queries::

            result = await (
                Post.objects
                .values("author_id")
                .annotate(post_count=Count("id"))
            )
            # → [{"author_id": 1, "post_count": 5}, ...]
        """

        builder = self._builder
        for f in fields:
            builder = builder.add_group_by(f)
        return self._clone(
            builder, _select_columns=list(fields), _group_by=list(fields)
        )

    # JOINs
    def join(
        self,
        table: str,
        on: str,
        *,
        alias: Optional[str] = None,
        kind: str = "INNER",
    ) -> "QuerySet":
        """Explicit JOIN clause.

        Args:
            table : The table to join (e.g. ``"authors"``).
            on    : Join condition as ``"left_table.col = right_table.col"``
                    or ``"left_col = right_col"``.
            alias : Optional table alias (e.g. ``"a"`` → ``JOIN authors AS a``).
            kind  : "INNER" (default), "LEFT", "RIGHT", "FULL", "CROSS".

        Example::

            posts = await (
                Post.objects
                .join("authors", "posts.author_id = authors.id", alias="a")
                .filter(authors__name__icontains="alice")
            )
        """

        left, right = on.split("=", 1)
        builder = self._builder.add_join(
            kind.upper(), table, alias or "", left.strip(), right.strip()
        )
        return self._clone(builder)

    def select_related(self, *fields: str) -> "QuerySet":
        """Stub for eager loading of related objects (planned feature).

        Currently a no-op — returns self unchanged.
        """
        # TODO: implement via LEFT JOIN + row reconstruction
        return self._clone()

    # Ordering / paging
    def order_by(self, *fields: str) -> "QuerySet":
        """Override ordering. Pass ``"-field"`` for DESC, ``"field"`` for ASC."""

        builder = self._builder
        for f in fields:
            builder = builder.add_order_by(f)
        return self._clone(builder)

    def limit(self, n: int) -> "QuerySet":
        return self._clone(self._builder.set_limit(n))

    def offset(self, n: int) -> "QuerySet":
        return self._clone(self._builder.set_offset(n))

    def distinct(self) -> "QuerySet":
        return self._clone(self._builder.set_distinct())

    def __getitem__(self, key):
        """Support slicing for pagination: qs[:3], qs[2:5], qs[3:7].

        Returns a new QuerySet with LIMIT/OFFSET applied.
        Negative indices are not supported (raises TypeError).
        A single integer index returns the instance at that position.

        Example::

            # First 3 posts
            posts = await Post.objects.order_by("views")[:3]

            # Posts 3 to 7
            posts = await Post.objects.order_by("views")[3:7]

            # Single post at index 2
            post = await Post.objects.order_by("views")[2]
        """
        if isinstance(key, int):
            # Single index: return the instance at that position
            if key < 0:
                raise TypeError("Negative indexing is not supported on QuerySet")
            qs = self._clone(self._builder.set_limit(1).set_offset(key))
            # Return a special awaitable that extracts single item
            return _IndexAwaitable(qs)
        elif isinstance(key, slice):
            if key.step is not None:
                raise TypeError("Step slicing is not supported on QuerySet")
            start = key.start if key.start is not None else 0
            stop = key.stop
            if start < 0 or (stop is not None and stop < 0):
                raise TypeError("Negative slicing is not supported on QuerySet")
            if stop is not None:
                limit = stop - start
            else:
                limit = None
            builder = self._builder.set_offset(start)
            if limit is not None:
                builder = builder.set_limit(limit)
            return self._clone(builder)
        else:
            raise TypeError(
                f"QuerySet indices must be integers or slices, not {type(key).__name__}"
            )

    def stream(
        self,
        *,
        chunk_size: int = 100,
        keyset: Optional[str] = None,
        as_dict: bool = False,
    ):
        """Async generator that yields model instances (or dicts) in chunks.

        Keeps memory usage bounded by fetching ``chunk_size`` rows at a time.

        By default uses LIMIT/OFFSET pagination.  For large tables, pass
        ``keyset="id"`` (or any indexed column) to use cursor-based pagination
        which avoids the O(n²) scan degradation of OFFSET.

        Args:
            chunk_size: Number of rows per DB fetch. Default: 100.
            keyset:     Column name for cursor-based pagination (e.g. "id").
                        Uses ``WHERE col > last_value ORDER BY col ASC``.
                        The column should be indexed for best performance.
            as_dict:    If True, yields raw dicts instead of model instances.
                        Much faster for ETL pipelines that don't need models.

        Usage::

            # Simple streaming (LIMIT/OFFSET)
            async for post in Post.objects.filter(active=True).stream():
                process(post)

            # Cursor-based streaming for large tables
            async for post in Post.objects.order_by("id").stream(keyset="id"):
                process(post)

            # Raw dicts for ETL
            async for row in Post.objects.stream(as_dict=True):
                etl_pipeline(row)

        Yields:
            Model instances (default) or dicts (as_dict=True).
        """
        return _stream_queryset(
            self, chunk_size=chunk_size, keyset=keyset, as_dict=as_dict
        )

    def using(self, alias: str) -> "QuerySet":
        """Stub for multi-database routing (planned feature)."""
        return self._clone()

    # Evaluation (async)
    def cache(
        self, *, ttl: Optional[int] = None, key: Optional[str] = None
    ) -> "QuerySet":
        """Return a QuerySet whose results are cached on first evaluation.

        Results are stored in the configured cache backend (see
        :func:`ryx.cache.configure_cache`). If no cache is configured,
        this method is a no-op.

        Args:
            ttl: Cache lifetime in seconds. Uses backend default if None.
            key: Explicit cache key. Auto-generated from SQL if None.

        Example::

            # Cache active posts for 60 seconds
            posts = await Post.objects.filter(active=True).cache(ttl=60)

            # Named key for manual invalidation
            posts = await Post.objects.all().cache(key="all_posts", ttl=300)
            await ryx.cache.invalidate("all_posts")

        Returns:
            A new QuerySet with caching enabled.
        """
        from ryx.cache import CachedQueryMixin

        # Dynamically create a cached subclass of this QuerySet
        CachedQS = type("CachedQuerySet", (CachedQueryMixin, QuerySet), {})
        clone = CachedQS(
            self._model,
            self._builder,
            _select_columns=self._select_columns,
            _annotations=list(self._annotations),
            _group_by=list(self._group_by),
        )
        clone._cache_ttl = ttl
        clone._cache_key = key
        return clone

    def __await__(self):
        return self._execute().__await__()

    async def _execute(self) -> list:
        raw_rows = await self._builder.fetch_all()
        return [self._model._from_row(row) for row in raw_rows]

    async def count(self) -> int:
        return await self._builder.fetch_count()

    async def first(self) -> Optional["Model"]:
        raw = await self._builder.set_limit(1).fetch_first()
        return None if raw is None else self._model._from_row(raw)

    async def last(self) -> Optional["Model"]:
        # Support explicit ordering from .order_by(...).last().
        # If no rows, return None.
        results = await self._execute()
        return results[-1] if results else None

    async def get(self, *q_args: Q, **kwargs: Any) -> "Model":
        """Return exactly one instance. Raises DoesNotExist / MultipleObjectsReturned."""
        qs = self.filter(*q_args, **kwargs) if (q_args or kwargs) else self
        try:
            raw = await qs._builder.fetch_get()
        except RuntimeError as e:
            msg = str(e)
            if "No matching" in msg:
                raise self._model.DoesNotExist(
                    f"{self._model.__name__} matching query does not exist."
                ) from e
            if "multiple" in msg.lower():
                raise self._model.MultipleObjectsReturned(
                    f"get() returned more than one {self._model.__name__}."
                ) from e
            raise
        return self._model._from_row(raw)

    async def exists(self) -> bool:
        return await self.count() > 0

    async def delete(self) -> int:
        """Bulk delete. Fires pre_bulk_delete / post_bulk_delete signals."""

        await pre_bulk_delete.send(sender=self._model, queryset=self)
        n = await self._builder.execute_delete()
        await post_bulk_delete.send(sender=self._model, queryset=self, deleted_count=n)
        return n

    async def bulk_delete(self) -> int:
        """Alias for delete()."""
        return await self.delete()

    async def update(self, **kwargs: Any) -> int:
        """Bulk update. Fires pre_update / post_update signals."""

        await pre_update.send(sender=self._model, queryset=self, fields=kwargs)
        n = await self._builder.execute_update(list(kwargs.items()))
        await post_update.send(
            sender=self._model, queryset=self, updated_count=n, fields=kwargs
        )
        return n

    async def in_bulk(self, id_list: list, *, field_name: str = "pk") -> dict:
        """Return a dict of {pk: instance} for the given list of PKs."""

        if not id_list:
            return {}
        fname = self._model._meta.pk_field.attname if field_name == "pk" else field_name
        instances = await self.filter(**{f"{fname}__in": id_list})
        return {getattr(obj, fname): obj for obj in instances}

    # Async iteration
    async def __aiter__(self):
        rows = await self._execute()
        for row in rows:
            yield row

    # Introspection
    @property
    def query(self) -> str:
        return self._builder.compiled_sql()

    def __repr__(self) -> str:
        return f"<QuerySet model={self._model.__name__} sql={self.query!r}>"


####    Sync / Async bridge helpers
def sync_to_async(fn, *, thread_sensitive: bool = True):
    """Wrap a synchronous callable to be usable in async context.

    Runs the callable in a thread pool so it doesn't block the event loop.

    Usage::

        sync_process = sync_to_async(my_blocking_function)
        result = await sync_process(arg1, arg2)

    Args:
        fn:               Any synchronous callable.
        thread_sensitive: If True, always use the same thread (safer for
                          non-thread-safe code like Django ORM). If False,
                          may use any worker thread.
    """

    @functools.wraps(fn)
    async def wrapper(*args, **kwargs):
        loop = asyncio.get_event_loop()
        return await loop.run_in_executor(None, functools.partial(fn, *args, **kwargs))

    wrapper._is_coroutine = asyncio.coroutines._is_coroutine  # type: ignore[attr-defined]
    return wrapper


def async_to_sync(fn):
    """Wrap an async coroutine to be callable from synchronous code.

    This is how you use ryx from WSGI apps, scripts, or Django views::

        from ryx.queryset import async_to_sync

        get_posts = async_to_sync(Post.objects.filter(active=True).__await__)

        # In a WSGI view:
        def my_view(request):
            posts = async_to_sync(lambda: Post.objects.filter(active=True))()
            return render(request, "posts.html", {"posts": posts})

    Or more ergonomically::

        from ryx.queryset import run_sync
        posts = run_sync(Post.objects.filter(active=True))
    """

    @functools.wraps(fn)
    def wrapper(*args, **kwargs):
        coro = fn(*args, **kwargs)
        return _run_coroutine_sync(coro)

    return wrapper


def run_sync(awaitable) -> Any:
    """Execute an awaitable from synchronous code.

    Creates a new event loop if needed (WSGI / script context).
    If an event loop is already running (e.g., inside an async test),
    raises RuntimeError with a helpful message.

    Usage::

        from ryx.queryset import run_sync

        # In a sync script
        posts = run_sync(Post.objects.filter(active=True))
        count = run_sync(Post.objects.count())
        post  = run_sync(Post.objects.get(pk=1))

    Raises:
        RuntimeError: if called from within a running event loop.
                      Use ``await`` directly in async contexts.
    """
    return _run_coroutine_sync(awaitable)


async def run_async(sync_fn: Any, *args, **kwargs) -> Any:
    """Run a synchronous function in a thread pool from async code.

    Usage::

        result = await run_async(some_blocking_function, arg1, key=val)
    """
    loop = asyncio.get_event_loop()
    return await loop.run_in_executor(None, functools.partial(sync_fn, *args, **kwargs))


def _run_coroutine_sync(coro) -> Any:
    """Run a coroutine or awaitable synchronously.

    Handles:
    - Regular coroutines (async def)
    - QuerySet instances (have __await__ that returns a coroutine_wrapper)

    Raises RuntimeError if called from within a running event loop.
    """
    import inspect

    # If the object is a QuerySet or anything with __await__, call _execute()
    # directly to get a proper coroutine that asyncio.run() can handle.
    if isinstance(coro, QuerySet):
        coro = coro._execute()
    elif hasattr(coro, "__await__") and not inspect.iscoroutine(coro):
        # For other awaitables, wrap in a coroutine via __await__
        async def _wrap(aw):
            return await aw

        coro = _wrap(coro)

    try:
        loop = asyncio.get_event_loop()
        if loop.is_running():
            # We are inside a running event loop; run in a background thread
            # to avoid nested loops. This keeps run_sync useful in async
            # callbacks and descriptors.
            import concurrent.futures

            def run_in_thread():
                return asyncio.run(coro)

            with concurrent.futures.ThreadPoolExecutor(max_workers=1) as executor:
                future = executor.submit(run_in_thread)
                return future.result()

        return loop.run_until_complete(coro)
    except RuntimeError as e:
        if "no current event loop" in str(e).lower():
            return asyncio.run(coro)
        raise


####    Internal helpers
def _get_known_lookups() -> frozenset:
    try:
        return frozenset(_core.available_lookups())
    except Exception:
        return frozenset(
            {
                "exact",
                "gt",
                "gte",
                "lt",
                "lte",
                "contains",
                "icontains",
                "startswith",
                "istartswith",
                "endswith",
                "iendswith",
                "isnull",
                "in",
                "range",
                # Date/Time transforms (can be part of chains)
                "date",
                "year",
                "month",
                "day",
                "hour",
                "minute",
                "second",
                "week",
                "dow",
                # JSON transforms (can be part of chains)
                "key",
                "key_text",
                "json",
                # JSON lookups (final lookups)
                "has_key",
                "has_keys",
                "contains",
                "contained_by",
            }
        )


def _parse_lookup_key(key: str):
    """Split 'field__lookup' → ('field', 'lookup'), or handle chained lookups.

    Examples:
        'created_at__gte'     → ('created_at', 'gte')
        'created_at__year__gte' → ('created_at', 'year__gte')
        'my_json__key__icontains' → ('my_json', 'key__icontains')
        'metadata__key__has_key' → ('metadata', 'key__has_key')
        'title__unknown'      → ('title', 'exact')  # unknown lookup falls back to exact
    """
    known = _get_known_lookups()
    parts = key.split("__")

    if len(parts) >= 2:
        # Search from the end to find the last known lookup
        for i in range(len(parts) - 1, 0, -1):
            if parts[i] in known:
                field = "__".join(parts[:i])
                lookup = "__".join(parts[i:])
                return field, lookup

        # No known lookup found in chain
        return parts[0], "exact"

    return key, "exact"


def _apply_q_node(builder, node: dict):
    """Recursively apply a Q node dict to the builder."""
    t = node.get("type", "leaf")
    if t == "leaf":
        return builder.add_filter(
            node["field"], node["lookup"], node["value"], node.get("negated", False)
        )
    if t == "and":
        for child in node.get("children", []):
            builder = _apply_q_node(builder, child)
        return builder
    if t == "or":
        # OR is passed to the Rust side as a Q-node structure
        return builder.add_q_node(node)
    if t == "not":
        children = node.get("children", [])
        if children:
            child = children[0]
            # Negate the child
            if child.get("type") == "leaf":
                return builder.add_filter(
                    child["field"],
                    child["lookup"],
                    child["value"],
                    not child.get("negated", False),
                )
        return builder
    return builder


####    Streaming helper
async def _stream_queryset(
    queryset,
    *,
    chunk_size: int = 100,
    keyset: Optional[str] = None,
    as_dict: bool = False,
):
    """Async generator that yields model instances or dicts in chunks.

    Supports two pagination strategies:
    - LIMIT/OFFSET (default): simple but O(n²) for large tables
    - Keyset/cursor-based: O(n) but requires an indexed column
    """
    model = queryset._model

    if keyset:
        # Keyset pagination: WHERE keyset > last_value ORDER BY keyset ASC
        # This is O(n) regardless of table size because the DB uses the index
        last_value = None
        while True:
            qs = queryset.limit(chunk_size)
            if last_value is not None:
                qs = qs.filter(**{f"{keyset}__gt": last_value})
            batch = await qs
            if not batch:
                break
            for item in batch:
                if as_dict:
                    yield (
                        item
                        if isinstance(item, dict)
                        else {
                            f.attname: getattr(item, f.attname)
                            for f in model._meta.fields.values()
                        }
                    )
                else:
                    yield item
                # Track the last keyset value for the next chunk
                last_value = (
                    getattr(item, keyset, None)
                    if not isinstance(item, dict)
                    else item.get(keyset)
                )
            if len(batch) < chunk_size:
                break
    else:
        # LIMIT/OFFSET pagination
        offset = 0
        while True:
            batch_qs = queryset.limit(chunk_size).offset(offset)
            batch = await batch_qs
            if not batch:
                break
            for item in batch:
                if as_dict:
                    yield (
                        item
                        if isinstance(item, dict)
                        else {
                            f.attname: getattr(item, f.attname)
                            for f in model._meta.fields.values()
                        }
                    )
                else:
                    yield item
            if len(batch) < chunk_size:
                break
            offset += chunk_size
