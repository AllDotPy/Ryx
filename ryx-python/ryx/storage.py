"""
Ryx ORM — Pluggable File Storage

A storage abstraction used by ``FileField`` / ``ImageField``. The default
backend is the local filesystem; users can implement :class:`Storage` for
S3, GCS, Azure Blob, etc. and register it globally with
:func:`configure_storage`.

Usage::

    from ryx.storage import configure_storage, FileSystemStorage

    configure_storage(FileSystemStorage(root="media/", base_url="/media/"))

Design:
  - All I/O methods are async (network-backed backends are first-class).
  - ``FileSystemStorage`` is the built-in local backend.
  - ``InMemoryStorage`` is provided for tests.
  - The global backend is set with ``configure_storage()`` and read with
    ``get_storage()``.
"""

from __future__ import annotations

import asyncio
import logging
import os
import secrets
from abc import ABC, abstractmethod
from pathlib import Path, PurePosixPath
from typing import Optional

logger = logging.getLogger("ryx.storage")


def _random_suffix(length: int = 7) -> str:
    """Return a short lowercase random hex suffix."""
    return secrets.token_hex(16)[:length]


def _split_name(name: str) -> tuple[str, str]:
    """Split ``name`` into ``(stem, extension)`` where extension includes the dot."""
    idx = name.rfind(".")
    if idx > 0:
        return name[:idx], name[idx:]
    return name, ""


def _validate_relative(name: str) -> None:
    """Reject absolute paths and ``..`` traversal."""
    if not name:
        raise ValueError("Storage path must not be empty")
    p = PurePosixPath(name)
    if p.is_absolute() or name.startswith("/"):
        raise ValueError(f"Storage path must be relative: {name}")
    if ".." in p.parts:
        raise ValueError(f"Storage path must not contain '..': {name}")


####
##      ABSTRACT STORAGE
#####
class Storage(ABC):
    """Protocol for Ryx file storage backends.

    Implement this to store files anywhere (local disk, S3, GCS, …).
    All I/O methods are async to support network-backed backends.
    """

    @abstractmethod
    async def save(self, name: str, content: bytes) -> str:
        """Write ``content`` under ``name`` and return the final stored name.

        Implementations must resolve collisions so an existing file is never
        silently overwritten.
        """

    @abstractmethod
    async def open(self, name: str) -> bytes:
        """Read and return the bytes stored under ``name``."""

    @abstractmethod
    async def delete(self, name: str) -> None:
        """Delete the file stored under ``name`` (idempotent)."""

    @abstractmethod
    async def exists(self, name: str) -> bool:
        """Return ``True`` if a file exists under ``name``."""

    @abstractmethod
    def url(self, name: str) -> str:
        """Return a public URL for ``name``."""

    @abstractmethod
    async def size(self, name: str) -> int:
        """Return the size of the stored file in bytes."""

    def path(self, name: str) -> str:
        """Return the local filesystem path for ``name``.

        Remote backends should raise ``NotImplementedError``.
        """
        raise NotImplementedError(
            f"Storage backend does not expose local paths: {name}"
        )

    async def get_available_name(self, name: str) -> str:
        """Resolve a collision-free name for ``name``.

        Default: if ``name`` is free, return it unchanged; otherwise append a
        random suffix before the extension (``a.jpg`` → ``a_a1b2c3d.jpg``).
        """
        if not await self.exists(name):
            return name
        stem, ext = _split_name(name)
        for _ in range(100):
            candidate = f"{stem}_{_random_suffix()}{ext}"
            if not await self.exists(candidate):
                return candidate
        return f"{stem}_{_random_suffix(16)}{ext}"

    async def listdir(self, path: str = "") -> list[tuple[str, str]]:
        """List entries under ``path`` as ``(name, kind)``.

        ``kind`` is ``"file"`` or ``"dir"``. Default: unsupported.
        """
        raise NotImplementedError("Storage backend does not support listdir")


####
##      FILESYSTEM STORAGE — LOCAL DEFAULT
#####
class FileSystemStorage(Storage):
    """Filesystem-backed storage rooted at ``root``.

    Args:
        root:      Root directory on disk (created on demand). Default: ``"media"``.
        base_url:  Public URL prefix (e.g. ``/media/``). Default: ``"/media/"``.
        file_permissions: Optional chmod applied to written files.
    """

    def __init__(
        self,
        root: str | os.PathLike = "media",
        base_url: str = "/media/",
        file_permissions: Optional[int] = None,
    ) -> None:
        self.root = Path(root)
        self.base_url = base_url
        self.file_permissions = file_permissions

    # Internal helpers
    def _safe_path(self, name: str) -> Path:
        _validate_relative(name)
        return self.root / name

    async def save(self, name: str, content: bytes) -> str:
        available = await self.get_available_name(name)
        full = self._safe_path(available)

        def _write() -> None:
            full.parent.mkdir(parents=True, exist_ok=True)
            with open(full, "wb") as fh:
                fh.write(content)
            if self.file_permissions is not None:
                os.chmod(full, self.file_permissions)

        await asyncio.to_thread(_write)
        return available

    async def open(self, name: str) -> bytes:
        full = self._safe_path(name)
        try:
            return await asyncio.to_thread(full.read_bytes)
        except FileNotFoundError as e:
            raise FileNotFoundError(f"File not found: {name}") from e

    async def delete(self, name: str) -> None:
        full = self._safe_path(name)
        try:
            await asyncio.to_thread(full.unlink)
        except FileNotFoundError:
            pass

    async def exists(self, name: str) -> bool:
        full = self._safe_path(name)
        return await asyncio.to_thread(full.is_file)

    def url(self, name: str) -> str:
        base = self.base_url.rstrip("/")
        return f"{base}/{name.lstrip('/')}"

    async def size(self, name: str) -> int:
        full = self._safe_path(name)
        try:
            return await asyncio.to_thread(lambda: full.stat().st_size)
        except FileNotFoundError as e:
            raise FileNotFoundError(f"File not found: {name}") from e

    def path(self, name: str) -> str:
        return str(self._safe_path(name))

    async def listdir(self, path: str = "") -> list[tuple[str, str]]:
        directory = self.root if not path else self._safe_path(path)

        def _list() -> list[tuple[str, str]]:
            out: list[tuple[str, str]] = []
            if not directory.exists():
                return out
            for entry in directory.iterdir():
                out.append((entry.name, "dir" if entry.is_dir() else "file"))
            return out

        return await asyncio.to_thread(_list)


####
##      IN-MEMORY STORAGE — TESTS
#####
class InMemoryStorage(Storage):
    """An in-memory storage backend. Useful for tests."""

    def __init__(self, base_url: str = "/media/") -> None:
        self.base_url = base_url
        self._data: dict[str, bytes] = {}
        self._lock = asyncio.Lock()

    async def save(self, name: str, content: bytes) -> str:
        _validate_relative(name)
        available = await self.get_available_name(name)
        async with self._lock:
            self._data[available] = bytes(content)
        return available

    async def open(self, name: str) -> bytes:
        async with self._lock:
            if name not in self._data:
                raise FileNotFoundError(f"File not found: {name}")
            return self._data[name]

    async def delete(self, name: str) -> None:
        async with self._lock:
            self._data.pop(name, None)

    async def exists(self, name: str) -> bool:
        async with self._lock:
            return name in self._data

    def url(self, name: str) -> str:
        base = self.base_url.rstrip("/")
        return f"{base}/{name.lstrip('/')}"

    async def size(self, name: str) -> int:
        async with self._lock:
            if name not in self._data:
                raise FileNotFoundError(f"File not found: {name}")
            return len(self._data[name])

    async def listdir(self, path: str = "") -> list[tuple[str, str]]:
        prefix = "" if not path else f"{path.rstrip('/')}/"
        async with self._lock:
            out: list[tuple[str, str]] = []
            for key in self._data:
                if key.startswith(prefix):
                    rest = key[len(prefix):]
                    if "/" not in rest:
                        out.append((rest, "file"))
            return out


####
##      GLOBAL STORAGE REGISTRY
#####
_default_storage: Optional[Storage] = None


def configure_storage(storage: Storage) -> None:
    """Configure the global default storage backend.

    Call once at application startup.

    Example::

        from ryx.storage import configure_storage, FileSystemStorage
        configure_storage(FileSystemStorage(root="media/", base_url="/media/"))
    """
    global _default_storage
    _default_storage = storage
    logger.info("Storage configured: backend=%s", type(storage).__name__)


def get_storage() -> Storage:
    """Return the configured storage backend.

    If none is configured, a default :class:`FileSystemStorage` rooted at
    ``RYX_STORAGE_ROOT`` (or ``"media"``) with base URL
    ``RYX_STORAGE_BASE_URL`` (or ``"/media/"``) is created lazily and cached.
    """
    global _default_storage
    if _default_storage is None:
        _default_storage = FileSystemStorage(
            root=os.getenv("RYX_STORAGE_ROOT", "media"),
            base_url=os.getenv("RYX_STORAGE_BASE_URL", "/media/"),
        )
        logger.debug("Using default FileSystemStorage at %s", _default_storage.root)
    return _default_storage


def default_storage() -> Storage:
    """Alias for :func:`get_storage`."""
    return get_storage()


def clear_storage() -> None:
    """Reset the global storage backend (mainly for tests)."""
    global _default_storage
    _default_storage = None
