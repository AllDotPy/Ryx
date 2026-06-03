from __future__ import annotations

# Import the compiled Rust extension directly to avoid circular import
import ryx.ryx_core as _core
import logging as _logging
import os

# Configure Ryx logging from RYX_LOG_LEVEL env var
_ryx_logger = _logging.getLogger("ryx")
_ryx_log_level = os.environ.get("RYX_LOG_LEVEL", "").strip().upper()
if _ryx_log_level in ("NO_LOG", "OFF", "SILENT"):
    _ryx_logger.disabled = True
    _ryx_logger.propagate = False
    _ryx_logger.addHandler(_logging.NullHandler())
elif _ryx_log_level:
    _ryx_log_level_num = getattr(_logging, _ryx_log_level, _logging.INFO)
    _ryx_log_handler = _logging.StreamHandler()
    _ryx_log_handler.setFormatter(
        _logging.Formatter("[ryx] %(levelname)s %(name)s: %(message)s")
    )
    _ryx_logger.addHandler(_ryx_log_handler)
    _ryx_logger.setLevel(_ryx_log_level_num)
    _ryx_logger.propagate = False


# ORM core
from ryx.models import Constraint, Index, Model
from ryx.fields import (
    ArrayField,
    AutoField,
    BigAutoField,
    BigIntField,
    BinaryField,
    BooleanField,
    CharField,
    DateField,
    DateTimeField,
    DecimalField,
    DurationField,
    EmailField,
    FloatField,
    ForeignKey,
    IntField,
    IPAddressField,
    JSONField,
    ManyToManyField,
    NullBooleanField,
    OneToOneField,
    PositiveIntField,
    SlugField,
    SmallAutoField,
    SmallIntField,
    TextField,
    TimeField,
    URLField,
    UUIDField,
)
from ryx.queryset import (
    Avg,
    Count,
    Max,
    Min,
    Q,
    QuerySet,
    RawAgg,
    Sum,
    async_to_sync,
    run_async,
    run_sync,
    sync_to_async,
)
from ryx.validators import (
    ChoicesValidator,
    EmailValidator,
    FunctionValidator,
    MaxLengthValidator,
    MaxValueValidator,
    MinLengthValidator,
    MinValueValidator,
    NotBlankValidator,
    NotNullValidator,
    RangeValidator,
    RegexValidator,
    URLValidator,
    ValidationError,
    Validator,
)
from ryx.signals import (
    Signal,
    receiver,
    pre_save,
    post_save,
    pre_delete,
    post_delete,
    pre_update,
    post_update,
    pre_bulk_delete,
    post_bulk_delete,
)
from ryx.transaction import transaction, get_active_transaction
from ryx.descriptors import (
    ForwardDescriptor,
    ReverseFKDescriptor,
    ManyToManyDescriptor,
    ReverseFKManager,
    ManyToManyManager,
)
from ryx.bulk import bulk_create, bulk_update, bulk_delete, stream
from ryx import cache as cache_module
from ryx.cache import (
    AbstractCache,
    MemoryCache,
    configure_cache,
    invalidate,
    invalidate_model,
    invalidate_all,
    get_cache,
)
from ryx.migrations.ddl import DDLGenerator, generate_schema_ddl, detect_backend
from ryx.migrations.autodetect import Autodetector
from ryx.exceptions import (
    RyxError,
    DatabaseError,
    DoesNotExist,
    MultipleObjectsReturned,
    PoolNotInitialized,
)


# Setup
async def setup(
    urls: str | dict, # str | dict to maintain backward.
    *,
    max_connections: int = 10,
    min_connections: int = 1,
    connect_timeout: int = 30,
    idle_timeout: int = 600,
    max_lifetime: int = 1800,
) -> None:
    """Initialize the ryx connection pool. Call once at startup."""
    
    # For old versions wrap the url with a dict
    if isinstance(urls, str):
        urls = {'default': urls}

    _ryx_logger.info(
        "Initializing pools: %s (max_conn=%d, min_conn=%d)",
        list(urls.keys()), max_connections, min_connections,
    )
    await _core.setup(
        urls,
        max_connections = max_connections,
        min_connections = min_connections,
        connect_timeout = connect_timeout,
        idle_timeout = idle_timeout,
        max_lifetime = max_lifetime,
    )
    _ryx_logger.info("Pools initialized: %s", list(urls.keys()))


def register_lookup(name: str, sql_template: str) -> None:
    """Register a custom lookup operator (process-global)."""
    _ryx_logger.debug("Register custom lookup: %s = %s", name, sql_template)
    _core.register_lookup(name, sql_template)


def available_lookups() -> list[str]:
    """Return all registered lookup names (built-in + custom)."""
    return _core.available_lookups()


def list_lookups() -> list[str]:
    """Return all built-in lookup names (for auto-discovery)."""
    return list(_core.list_lookups())

def list_aliases() -> list[str]:
    """Return all available databases aliases"""
    return _core.list_aliases()


def available_transforms() -> list[str]:
    """Return all built-in transform names (for auto-discovery)."""
    return list(_core.list_transforms())


def is_connected(db_alias: str = 'default') -> bool:
    return _core.is_connected(db_alias)


def pool_stats() -> dict:
    return _core.pool_stats()


def lookup(name: str):
    """Decorator shortcut for registering a lookup."""

    def decorator(sql_template_or_fn):
        if isinstance(sql_template_or_fn, str):
            register_lookup(name, sql_template_or_fn)
            return sql_template_or_fn
        doc = sql_template_or_fn.__doc__
        if doc:
            register_lookup(name, doc.strip())
        return sql_template_or_fn

    return decorator


__version__: str = _core.__version__

__all__ = [
    # Setup
    "setup",
    "register_lookup",
    "available_lookups",
    "is_connected",
    "pool_stats",
    "lookup",
    "list_lookups",
    "list_transforms",
    # Model
    "Model",
    "Index",
    "Constraint",
    # Fields
    "ArrayField",
    "AutoField",
    "BigAutoField",
    "BigIntField",
    "BinaryField",
    "BooleanField",
    "CharField",
    "DateField",
    "DateTimeField",
    "DecimalField",
    "DurationField",
    "EmailField",
    "FloatField",
    "ForeignKey",
    "IntField",
    "IPAddressField",
    "JSONField",
    "ManyToManyField",
    "NullBooleanField",
    "OneToOneField",
    "PositiveIntField",
    "SlugField",
    "SmallAutoField",
    "SmallIntField",
    "TextField",
    "TimeField",
    "URLField",
    "UUIDField",
    # QuerySet
    "QuerySet",
    "Q",
    # Aggregates
    "Count",
    "Sum",
    "Avg",
    "Min",
    "Max",
    "RawAgg",
    # Sync/async helpers
    "sync_to_async",
    "async_to_sync",
    "run_sync",
    "run_async",
    # Validators
    "ValidationError",
    "Validator",
    "FunctionValidator",
    "NotNullValidator",
    "NotBlankValidator",
    "MaxLengthValidator",
    "MinLengthValidator",
    "MinValueValidator",
    "MaxValueValidator",
    "RangeValidator",
    "RegexValidator",
    "EmailValidator",
    "URLValidator",
    "ChoicesValidator",
    # Signals
    "Signal",
    "receiver",
    "pre_save",
    "post_save",
    "pre_delete",
    "post_delete",
    "pre_update",
    "post_update",
    "pre_bulk_delete",
    "post_bulk_delete",
    # Exceptions
    "ryxError",
    "DatabaseError",
    "DoesNotExist",
    "MultipleObjectsReturned",
    "PoolNotInitialized",
    "ValidationError",
    # Transactions
    "transaction",
    "get_active_transaction",
    # Descriptors / relations
    "ForwardDescriptor",
    "ReverseFKDescriptor",
    "ManyToManyDescriptor",
    "ReverseFKManager",
    "ManyToManyManager",
    # Bulk operations
    "bulk_create",
    "bulk_update",
    "bulk_delete",
    "stream",
    # Cache
    "AbstractCache",
    "MemoryCache",
    "configure_cache",
    "invalidate",
    "invalidate_model",
    "invalidate_all",
    "get_cache",
    # Migrations
    "DDLGenerator",
    "generate_schema_ddl",
    "detect_backend",
    "Autodetector",
    # Version
    "__version__",
]

# ---
# Optional auto-initialize (can be disabled with RYX_AUTO_INITIALIZE=0|no|false|n)
# ---
_AUTO_INIT_DONE = False


def _should_auto_init() -> bool:
    return os.getenv("RYX_AUTO_INITIALIZE", "1").lower() not in ("0", "false", "n", "no")


def _discover_urls_from_env() -> dict:
    urls = {}
    for key, val in os.environ.items():
        if key.startswith("RYX_DB_") and key.endswith("_URL"):
            alias = key.removeprefix("RYX_DB_").removesuffix("_URL").lower()
            urls[alias] = val
    if "default" not in urls:
        env_url = os.environ.get("RYX_DATABASE_URL")
        if env_url:
            urls["default"] = env_url
    return urls


def _discover_config_file():
    try:
        from ryx.cli.config_loader import find_config_file, load_config_file
    except Exception:
        return {}
    path = find_config_file()
    if not path:
        return {}
    try:
        return load_config_file(path) or {}
    except Exception:
        return {}


def _auto_setup():
    global _AUTO_INIT_DONE
    if _AUTO_INIT_DONE:
        return
    if not _should_auto_init():
        _ryx_logger.debug("Auto-init disabled via RYX_AUTO_INITIALIZE")
        return

    urls = _discover_urls_from_env()
    pool_cfg = {}
    cfg = _discover_config_file()
    if cfg:
        urls.update(cfg.get("urls", {}) or {})
        pool_cfg = cfg.get("pool", {}) or {}

    if not urls:
        _ryx_logger.debug("No URLs found — auto-init skipped")
        return

    _ryx_logger.info("Auto-initializing with URLs: %s", list(urls.keys()))

    try:
        import asyncio

        async def _do():
            await setup(
                urls,
                max_connections = pool_cfg.get("max_conn", 10),
                min_connections = pool_cfg.get("min_conn", 1),
                connect_timeout = pool_cfg.get("connect_timeout", 30),
                idle_timeout = pool_cfg.get("idle_timeout", 600),
                max_lifetime = pool_cfg.get("max_lifetime", 1800),
            )

        try:
            loop = asyncio.get_running_loop()
        except RuntimeError:
            loop = None

        if loop and loop.is_running():
            # In an already running loop, avoid blocking; user can call setup manually.
            return
        
        # No running loop: create a temporary loop to init pools.
        loop = asyncio.new_event_loop()
        asyncio.set_event_loop(loop)
        loop.run_until_complete(_do())
        loop.close()
        asyncio.set_event_loop(None)
        _AUTO_INIT_DONE = True
    except Exception as e:
        # Fail silently to avoid breaking imports; user can call setup manually.
        print(e)
        pass


_auto_setup()
