"""
Ryx ORM — Query Result Cache Layer

A pluggable, optional caching layer that wraps QuerySet evaluation and
stores results in a configurable backend.

Usage:

  # 1. Configure a cache backend at startup
  from ryx.cache import configure_cache, MemoryCache
  configure_cache(MemoryCache(max_size=1000, ttl=300))

  # 2. Use .cache() on any QuerySet
  posts = await Post.objects.filter(active=True).cache(ttl=60)
  posts = await Post.objects.filter(active=True).cache(key="active_posts")

  # 3. Invalidate manually
  from ryx.cache import invalidate, invalidate_model
  invalidate("active_posts")
  invalidate_model(Post)     # removes all cached queries for Post

Design:
  - The cache is pluggable: implement AbstractCache to use Redis, memcached,
    or any other backend.
  - MemoryCache is a built-in in-process LRU cache (good for dev/testing).
  - Cache keys are auto-generated from the compiled SQL + bound values unless
    the user specifies an explicit key.
  - Signals (post_save, post_delete) auto-invalidate per-model caches when
    the ``auto_invalidate`` option is set on configure_cache().
"""

from __future__ import annotations

import asyncio
import hashlib
import json
import time
from abc import ABC, abstractmethod
from collections import OrderedDict
from typing import Any, Optional


####
##      ABSTRACT CACHE BACKEND
#####
class AbstractCache(ABC):
    """Protocol for Ryx cache backends.

    Implement this to use Redis, memcached, or any other store.
    All methods are async to allow network-backed backends.
    """

    @abstractmethod
    async def get(self, key: str) -> Optional[Any]:
        """Return cached value or None if missing/expired."""

    @abstractmethod
    async def set(self, key: str, value: Any, ttl: Optional[int] = None) -> None:
        """Store a value with optional TTL (seconds)."""

    @abstractmethod
    async def delete(self, key: str) -> None:
        """Remove a single cached entry."""

    @abstractmethod
    async def delete_many(self, keys: list[str]) -> None:
        """Remove multiple entries."""

    @abstractmethod
    async def clear(self) -> None:
        """Remove all cached entries."""

    @abstractmethod
    async def keys(self, pattern: str = "*") -> list[str]:
        """Return all matching cache keys."""


#### 
##      MEMORY CACHE — BUILT-IN LRU IN PROCESS CHACHE
#####
class MemoryCache(AbstractCache):
    """Thread-safe in-process LRU cache with TTL support.

    Good for development, testing, and single-process deployments.
    Not shared across processes — use RedisCache for multi-process setups.

    Args:
        max_size: Maximum number of entries. Oldest entries are evicted
                  when the limit is reached (LRU eviction).
        ttl: Default TTL in seconds. ``None`` means no expiry.
    """

    def __init__(self, max_size: int = 1000, ttl: Optional[int] = 300) -> None:
        self._max_size    = max_size
        self._default_ttl = ttl
        self._store: OrderedDict[str, tuple[Any, Optional[float]]] = OrderedDict()
        self._lock = asyncio.Lock()

    async def get(self, key: str) -> Optional[Any]:
        async with self._lock:
            if key not in self._store:
                return None
            value, expires_at = self._store[key]
            if expires_at is not None and time.monotonic() > expires_at:
                del self._store[key]
                return None
            # LRU: move to end on access
            self._store.move_to_end(key)
            return value

    async def set(self, key: str, value: Any, ttl: Optional[int] = None) -> None:
        effective_ttl = ttl if ttl is not None else self._default_ttl
        expires_at    = time.monotonic() + effective_ttl if effective_ttl else None

        async with self._lock:
            if key in self._store:
                self._store.move_to_end(key)
            self._store[key] = (value, expires_at)
            # Evict oldest entries when over capacity
            while len(self._store) > self._max_size:
                self._store.popitem(last=False)

    async def delete(self, key: str) -> None:
        async with self._lock:
            self._store.pop(key, None)

    async def delete_many(self, keys: list[str]) -> None:
        async with self._lock:
            for key in keys:
                self._store.pop(key, None)

    async def clear(self) -> None:
        async with self._lock:
            self._store.clear()

    async def keys(self, pattern: str = "*") -> list[str]:
        import fnmatch
        async with self._lock:
            now = time.monotonic()
            return [
                k for k, (_, exp) in self._store.items()
                if (exp is None or now < exp)
                and fnmatch.fnmatch(k, pattern)
            ]

    def size(self) -> int:
        """Return number of currently stored entries (including expired)."""
        return len(self._store)


####    Global cache registry
_cache_backend: Optional[AbstractCache] = None
_auto_invalidate: bool = False


def configure_cache(
    backend:          AbstractCache,
    auto_invalidate:  bool = True,
) -> None:
    """Configure the global cache backend.

    Call this once at application startup, after ``Ryx.setup()``.

    Args:
        backend: An AbstractCache implementation (e.g. MemoryCache).
        auto_invalidate: If True, automatically invalidate all cached queries
                         for a model when post_save / post_delete fires.
                         Default: True.

    Example::

        from ryx.cache import configure_cache, MemoryCache
        configure_cache(MemoryCache(max_size=500, ttl=60))
    """
    global _cache_backend, _auto_invalidate
    _cache_backend   = backend
    _auto_invalidate = auto_invalidate

    if auto_invalidate:
        _register_invalidation_signals()


def get_cache() -> Optional[AbstractCache]:
    """Return the configured cache backend, or None if not configured."""
    return _cache_backend


####    Cache key generation
def make_cache_key(model_name: str, sql: str, values: list) -> str:
    """Generate a stable cache key from a query.

    The key is a SHA-256 hash of ``{model_name}:{sql}:{values_json}``
    prefixed with the model name for easy per-model invalidation.

    Args:
        model_name: The model class name (used for prefix).
        sql:        The compiled SQL string.
        values:     The bound parameter values.

    Returns:
        A string key like ``"Ryx:Post:a3f1c9d2..."``
    """

    payload = json.dumps({"sql": sql, "values": values}, sort_keys=True, default=str)
    digest  = hashlib.sha256(payload.encode()).hexdigest()[:16]
    return f"ryx:{model_name}:{digest}"


####    Public invalidation API
async def invalidate(key: str) -> None:
    """Remove a specific cache entry by key.

    Args:
        key: The cache key to remove (use the same key passed to ``.cache()``).
    """
    if _cache_backend:
        await _cache_backend.delete(key)


async def invalidate_model(model: type) -> None:
    """Invalidate all cached queries for a specific model class.

    Removes all entries whose keys match the prefix ``Ryx:{ModelName}:``.

    Args:
        model: The Model class whose cached queries should be removed.
    """
    if not _cache_backend:
        return
    prefix  = f"ryx:{model.__name__}:*"
    keys    = await _cache_backend.keys(prefix)
    if keys:
        await _cache_backend.delete_many(keys)


async def invalidate_all() -> None:
    """Clear the entire cache."""
    if _cache_backend:
        await _cache_backend.clear()


####
##      CACHED QUERYSET MIXIN  —  used by `QuerySet.cache()`
#####
class CachedQueryMixin:
    """Adds ``.cache(ttl, key)`` to a QuerySet instance.

    This is a mixin applied dynamically by ``QuerySet.cache()``.
    It overrides ``_execute()`` to check/populate the cache.
    """

    _cache_ttl: Optional[int] = None
    _cache_key: Optional[str] = None

    async def _execute(self) -> list:
        """Execute query with cache check."""
        backend = get_cache()
        if not backend:
            # No cache configured — fall through to DB
            return await super()._execute()  # type: ignore[misc]

        # Determine the cache key
        sql = self._builder.compiled_sql()          # type: ignore[attr-defined]
        model_name = self._model.__name__           # type: ignore[attr-defined]
        key = self._cache_key or make_cache_key(model_name, sql, [])

        # Try cache first
        cached = await backend.get(key)
        if cached is not None:
            return cached

        # Cache miss → hit DB
        result = await super()._execute()  # type: ignore[misc]

        # Serialise model instances to plain dicts for caching
        # (model instances are not directly serialisable)
        serialised = [inst.__dict__.copy() for inst in result]
        await backend.set(key, serialised, ttl=self._cache_ttl)

        return result


####    Auto-invalidation via signals
def _register_invalidation_signals() -> None:
    """Connect signal handlers that invalidate caches on model mutations."""

    from ryx.signals import post_save, post_delete, post_update, post_bulk_delete

    async def _on_mutate(sender, **kwargs):
        await invalidate_model(sender)

    # Use weak=False so the handlers aren't garbage-collected
    post_save.connect(_on_mutate, weak = False)
    post_delete.connect(_on_mutate, weak = False)
    post_update.connect(_on_mutate, weak = False)
    post_bulk_delete.connect(_on_mutate, weak = False)
