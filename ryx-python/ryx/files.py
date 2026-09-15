"""
Ryx ORM — FieldFile value object

A ``FieldFile`` is the value type exposed by ``FileField`` / ``ImageField``
attributes. It wraps the stored file name and knows how to read, delete,
and URL-address the underlying file via a :class:`~ryx.storage.Storage`.

Usage::

    item.avatar = b"...bytes..."          # staged
    await item.save()                     # committed to storage

    await item.avatar.save("x.png", b"")  # explicit immediate commit
    data = await item.avatar.read()
    url  = item.avatar.url
    await item.avatar.delete()
"""

from __future__ import annotations

import logging
from typing import TYPE_CHECKING, Any, Optional

from ryx.storage import Storage, get_storage

if TYPE_CHECKING:
    from ryx.models import Model
    from ryx.fields import FileField

logger = logging.getLogger("ryx.files")


class FieldFile:
    """A file attached to a model instance.

    Attributes:
        instance: The owning model instance.
        field:    The ``FileField`` this value belongs to.
        storage:  The storage backend used to read/write the file.
        name:     The stored name/path (``None`` if empty).
    """

    def __init__(
        self,
        instance: Optional["Model"],
        field: "FileField",
        name: Optional[str] = None,
    ) -> None:
        self.instance = instance
        self.field = field
        self.storage: Storage = field.storage or get_storage()
        self.name: Optional[str] = name
        # A staged file not yet written to storage.
        self._committed: bool = True
        self._pending: Optional[tuple[str, bytes]] = None

    # Dunder helpers
    def __bool__(self) -> bool:
        return bool(self.name)

    def __eq__(self, other: Any) -> bool:
        if isinstance(other, FieldFile):
            return self.name == other.name
        if isinstance(other, str):
            return (self.name or "") == other
        return NotImplemented

    def __hash__(self) -> int:
        return hash(self.name)

    def __repr__(self) -> str:
        return f"<FieldFile: {self.name!r}>"

    # Properties
    @property
    def url(self) -> str:
        """Public URL for the stored file (empty string if no name)."""
        return self.storage.url(self.name) if self.name else ""

    @property
    def path(self) -> str:
        """Local filesystem path (raises for remote backends)."""
        if not self.name:
            raise ValueError("FieldFile has no name")
        return self.storage.path(self.name)

    # I/O
    async def save(self, name: str, content: bytes) -> None:
        """Immediately write ``content`` to storage under ``name``.

        Updates this FieldFile's ``name`` to the final stored name and clears
        any staged content.
        """
        stored = await self.storage.save(name, content)
        self.name = stored
        self._committed = True
        self._pending = None

    async def open(self) -> bytes:
        """Read and return the stored bytes."""
        if self._pending is not None:
            return self._pending[1]
        if not self.name:
            raise ValueError("FieldFile has no name")
        return await self.storage.open(self.name)

    async def read(self) -> bytes:
        """Alias for :meth:`open`."""
        return await self.open()

    async def delete(self, save: bool = True) -> None:
        """Delete the file from storage.

        Args:
            save: If True and the file belongs to a saved instance, persist
                  the cleared value on the model.
        """
        if self._pending is not None:
            self._pending = None
            self._committed = True
            self.name = None
            return
        if self.name:
            await self.storage.delete(self.name)
        self.name = None
        self._committed = True
        if save and self.instance is not None and self.field is not None:
            setattr(self.instance, self.field.attname, None)
            if getattr(self.instance, "pk", None) is not None:
                await self.instance.save(update_fields=[self.field.attname])

    async def exists(self) -> bool:
        """Return ``True`` if the file exists in storage."""
        if self._pending is not None:
            return True
        if not self.name:
            return False
        return await self.storage.exists(self.name)

    async def size(self) -> int:
        """Return the stored file size in bytes."""
        if self._pending is not None:
            return len(self._pending[1])
        if not self.name:
            return 0
        return await self.storage.size(self.name)

    # Internal: commit a staged file via the field's storage
    async def _commit(self) -> None:
        """Write staged content to storage (called by FileField.before_save)."""
        if self._pending is None:
            return
        name, content = self._pending
        stored = await self.storage.save(name, content)
        self.name = stored
        self._pending = None
        self._committed = True
