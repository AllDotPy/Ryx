from __future__ import annotations

from typing import Dict, List, Type, Callable
from ryx.cli.commands.base import Command


_COMMAND_REGISTRY: Dict[str, Type[Command]] = {}
_INITIALIZED = False


def register_command(cls: Type[Command]) -> Type[Command]:
    """Decorator to register a command class."""
    _COMMAND_REGISTRY[cls.name] = cls
    return cls


def get_commands() -> Dict[str, Type[Command]]:
    """Get all registered commands."""
    if not _INITIALIZED:
        _discover_commands()
    return _COMMAND_REGISTRY.copy()


def _discover_commands() -> None:
    """Auto-discover all commands from the commands package."""
    global _INITIALIZED

    if _INITIALIZED:
        return

    from ryx.cli.commands import (
        migrate,
        makemigrations,
        showmigrations,
        sqlmigrate,
        flush,
        shell,
        dbshell,
        version,
        inspectdb,
    )

    _INITIALIZED = True


def clear_registry() -> None:
    """Clear the command registry (mainly for testing)."""
    global _INITIALIZED
    _COMMAND_REGISTRY.clear()
    _INITIALIZED = False
