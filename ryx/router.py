"""
Ryx ORM — Database Router

A router allows you to automatically route queries to different databases
based on the model, the operation (read vs write), or other hints.
"""

from __future__ import annotations
from typing import Any, Optional, TYPE_CHECKING

if TYPE_CHECKING:
    from ryx.models import Model


class BaseRouter:
    """
    Base class for database routers.
    Override these methods to implement custom routing logic.

    Returning None tells Ryx to fall back to the model's Meta.database
    or the global 'default' database.
    """

    def db_for_read(self, model: type[Model], **hints: Any) -> Optional[str]:
        """Return the alias of the database to use for read operations."""
        return None

    def db_for_write(self, model: type[Model], **hints: Any) -> Optional[str]:
        """Return the alias of the database to use for write operations."""
        return None

    def allow_migrate(self, db: str, app_label: str, model_name: str) -> Optional[bool]:
        """Return True/False to allow/disallow migrations on a specific DB."""
        return None


# Global router instance
_router: Optional[BaseRouter] = None


def set_router(router: BaseRouter) -> None:
    """Set the global router for the application."""
    global _router
    _router = router


def get_router() -> Optional[BaseRouter]:
    """Retrieve the currently configured router."""
    return _router
