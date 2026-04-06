from __future__ import annotations

import os
from pathlib import Path
from typing import Any, Dict, Optional


CONFIG_FILES = [
    "ryx.yaml",
    "ryx.yml",
    "ryx.toml",
    "ryx.json",
]


def find_config_file(search_paths: list[Path] = None) -> Optional[Path]:
    """Find the first existing config file from standard locations."""
    if search_paths is None:
        search_paths = [Path.cwd()]

    for base in search_paths:
        for filename in CONFIG_FILES:
            path = base / filename
            if path.exists():
                return path

    return None


def load_config_file(path: Path) -> Dict[str, Any]:
    """Load configuration from a YAML/TOML/JSON file."""
    import json

    ext = path.suffix.lower()

    if ext in (".yaml", ".yml"):
        try:
            import yaml

            with open(path, "r") as f:
                return yaml.safe_load(f) or {}
        except ImportError:
            raise ImportError(
                "PyYAML is required for .yaml config files. Install with: pip install pyyaml"
            )

    elif ext == ".toml":
        try:
            import tomllib
        except ImportError:
            # Python 3.11+ has tomllib, older need tomli
            try:
                import tomli as tomllib
            except ImportError:
                raise ImportError(
                    "tomli is required for .toml config files. Install with: pip install tomli"
                )

        with open(path, "rb") as f:
            return tomllib.load(f)

    elif ext == ".json":
        with open(path, "r") as f:
            return json.load(f)

    else:
        raise ValueError(f"Unsupported config file format: {ext}")


class ConfigLoader:
    """Loads configuration from multiple sources with precedence order."""

    def __init__(self):
        self._config: Dict[str, Any] = {}
        self._loaded = False

    def load(
        self, path: Optional[Path] = None, env: Optional[str] = None
    ) -> Dict[str, Any]:
        """Load configuration from file and environment.

        Precedence (highest to lowest):
        1. Config file (ryx.yaml/yml/toml)
        2. Environment variables
        3. Default values

        Args:
            path: Explicit config file path
            env: Environment name (dev, prod, test) for multi-env configs
        """
        if path is None:
            path = find_config_file()

        if path:
            self._config = load_config_file(path)

        # Load environment-specific config if specified
        if env and env in self._config:
            env_config = self._config.pop(env)
            self._merge_config(env_config)

        self._loaded = True
        return self._config

    def _merge_config(self, other: Dict[str, Any]) -> None:
        """Deep merge another config into the current config."""

        def merge(target: dict, source: dict):
            for key, value in source.items():
                if (
                    key in target
                    and isinstance(target[key], dict)
                    and isinstance(value, dict)
                ):
                    merge(target[key], value)
                else:
                    target[key] = value

        merge(self._config, other)

    def get(self, key: str, default: Any = None) -> Any:
        """Get a config value using dot notation (e.g., 'database.url')."""
        keys = key.split(".")
        value = self._config
        for k in keys:
            if isinstance(value, dict):
                value = value.get(k)
            else:
                return default
        return value if value is not None else default

    @property
    def database_url(self) -> Optional[str]:
        """Get database URL from config."""
        # Try various common paths
        return (
            self.get("database.url")
            or self.get("database.url")
            or self.get("url")
            or os.environ.get("RYX_DATABASE_URL")
        )

    @property
    def debug(self) -> bool:
        """Get debug setting."""
        return self.get("debug", False)

    @property
    def pool_settings(self) -> Dict[str, Any]:
        """Get connection pool settings."""
        return {
            "max_connections": self.get("database.pool.max_connections", 10),
            "min_connections": self.get("database.pool.min_connections", 1),
            "connect_timeout": self.get("database.pool.connect_timeout", 30),
            "idle_timeout": self.get("database.pool.idle_timeout", 600),
            "max_lifetime": self.get("database.pool.max_lifetime", 1800),
        }


# Global loader instance
_loader: Optional[ConfigLoader] = None


def get_loader() -> ConfigLoader:
    """Get the global config loader instance."""
    global _loader
    if _loader is None:
        _loader = ConfigLoader()
    return _loader


def load_config(
    path: Optional[Path] = None, env: Optional[str] = None
) -> Dict[str, Any]:
    """Convenience function to load configuration."""
    return get_loader().load(path, env)


__all__ = [
    "ConfigLoader",
    "find_config_file",
    "load_config_file",
    "load_config",
    "get_loader",
]
