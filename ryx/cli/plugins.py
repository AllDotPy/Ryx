from __future__ import annotations

from abc import ABC, abstractmethod
from typing import TYPE_CHECKING, Any, Dict, List, Type

if TYPE_CHECKING:
    from ryx.cli.commands.base import Command


class Plugin(ABC):
    """Abstract base class for ryx CLI plugins.

    Plugins can register additional commands, modify configuration,
    or add custom behavior to the CLI.

    Usage:
        class MyPlugin(Plugin):
            name = "my_plugin"

            def get_commands(self) -> List[Type[Command]]:
                from ryx.cli.commands.base import Command
                return [MyCustomCommand]

            def on_load(self) -> None:
                print("Plugin loaded!")
    """

    name: str = ""
    version: str = ""
    description: str = ""

    @abstractmethod
    def get_commands(self) -> List[Type["Command"]]:
        """Return a list of Command classes to register.

        Returns:
            List of Command subclasses to add to the CLI.
        """

    def on_load(self) -> None:
        """Called when the plugin is loaded.

        Use this for initialization, checking dependencies, etc.
        """

    def on_unload(self) -> None:
        """Called when the plugin is unloaded (if applicable)."""


class PluginManager:
    """Manages plugin loading and command registration."""

    def __init__(self):
        self._plugins: Dict[str, Plugin] = {}
        self._loaded = False

    def register(self, plugin: Plugin) -> None:
        """Register a plugin instance."""
        if not plugin.name:
            raise ValueError("Plugin must have a name")
        self._plugins[plugin.name] = plugin
        plugin.on_load()

    def get(self, name: str) -> Plugin:
        """Get a plugin by name."""
        return self._plugins[name]

    def list_plugins(self) -> List[Plugin]:
        """List all loaded plugins."""
        return list(self._plugins.values())

    def get_commands(self) -> List[Type["Command"]]:
        """Get all commands from all plugins."""
        commands = []
        for plugin in self._plugins.values():
            commands.extend(plugin.get_commands())
        return commands


# Global plugin manager
_manager: PluginManager = None


def get_plugin_manager() -> PluginManager:
    """Get the global plugin manager instance."""
    global _manager
    if _manager is None:
        _manager = PluginManager()
    return _manager


def load_plugins_from_settings() -> None:
    """Load plugins defined in ryx_settings.CLI_PLUGINS."""
    try:
        import importlib

        mod = importlib.import_module("ryx_settings")
    except ImportError:
        return

    plugins_config = getattr(mod, "CLI_PLUGINS", None)
    if not plugins_config:
        return

    manager = get_plugin_manager()

    for plugin_path in plugins_config:
        try:
            if isinstance(plugin_path, str):
                # Import path like "myapp.plugins.MyPlugin"
                module_path, class_name = plugin_path.rsplit(".", 1)
                module = importlib.import_module(module_path)
                plugin_cls = getattr(module, class_name)
                plugin = plugin_cls()
            elif isinstance(plugin_path, type) and issubclass(plugin_path, Plugin):
                plugin = plugin_path()
            else:
                continue

            manager.register(plugin)
        except Exception as e:
            print(f"[WARNING] Failed to load plugin {plugin_path}: {e}")


def load_plugins_from_entry_points() -> None:
    """Load plugins registered via entry points (setuptools/pyproject.toml).

    Entry point group: ryx_cli_plugins
    """
    try:
        from importlib.metadata import entry_points
    except ImportError:
        # Python < 3.10
        from importlib_metadata import entry_points

    try:
        eps = entry_points()
        ryx_eps = eps.get("ryx_cli_plugins", []) or eps.select(group="ryx_cli_plugins")
    except Exception:
        return

    manager = get_plugin_manager()

    for ep in ryx_eps:
        try:
            plugin_cls = ep.load()
            plugin = plugin_cls()
            manager.register(plugin)
        except Exception as e:
            print(f"[WARNING] Failed to load plugin from {ep.name}: {e}")


def discover_and_load_plugins() -> None:
    """Discover and load all plugins from known sources."""
    load_plugins_from_settings()
    load_plugins_from_entry_points()


__all__ = [
    "Plugin",
    "PluginManager",
    "get_plugin_manager",
    "load_plugins_from_settings",
    "load_plugins_from_entry_points",
    "discover_and_load_plugins",
]
