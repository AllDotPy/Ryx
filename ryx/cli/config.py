from __future__ import annotations

import os
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Dict, Optional

from ryx.cli.config_loader import get_loader, load_config


@dataclass
class Config:
    """Global CLI configuration.

    Configuration is resolved from (in order of precedence):
    1. CLI arguments (--url, --settings)
    2. Config file (ryx.yaml, ryx.yml, ryx.toml, ryx.json)
    3. Environment variables (RYX_DATABASE_URL)
    4. Python module (ryx_settings.py)
    """

    url: Optional[str] = None
    settings: str = "ryx_settings"
    debug: bool = False
    verbose: bool = False

    # Config file path
    config_file: Optional[Path] = None

    # Environment (dev, prod, test) for multi-env configs
    env: Optional[str] = None

    # Pool settings (passed to ryx.setup)
    max_connections: int = 10
    min_connections: int = 1
    connect_timeout: int = 30
    idle_timeout: int = 600
    max_lifetime: int = 1800

    # Loaded raw config (from YAML/TOML)
    _raw_config: Dict[str, Any] = field(default_factory=dict, repr=False)

    @classmethod
    def from_args(cls, args) -> "Config":
        """Create config from parsed argparse.Namespace."""
        config = cls()
        config.url = getattr(args, "url", None)
        config.settings = getattr(args, "settings", "ryx_settings")
        config.debug = getattr(args, "debug", False)
        config.verbose = getattr(args, "verbose", False)

        # Load config file if specified
        config_file = getattr(args, "config_file", None)
        if config_file:
            config.config_file = Path(config_file)

        config.env = getattr(args, "env", None)

        return config

    def _load_file_config(self) -> None:
        """Load configuration from YAML/TOML file."""
        if self._raw_config:
            return  # Already loaded

        # Try explicit config file path first
        if self.config_file and self.config_file.exists():
            self._raw_config = load_config(self.config_file, self.env)
            return

        # Try default config files
        try:
            loader = get_loader()
            self._raw_config = loader.load(env=self.env)
        except Exception:
            pass

    def resolve_url(self) -> str:
        """Resolve database URL from CLI args, env var, settings module, or config file."""
        # CLI arg takes precedence
        if self.url:
            return self.url

        # Try config file
        self._load_file_config()
        file_url = self._raw_config.get("database", {}).get("url")
        if file_url:
            return file_url

        # Environment variable
        url = os.environ.get("RYX_DATABASE_URL")
        if url:
            return url

        # Settings module
        settings_mod = self.settings
        if settings_mod:
            try:
                import importlib

                mod = importlib.import_module(settings_mod)
                url = getattr(mod, "DATABASE_URL", None)
                if url:
                    return url
            except ImportError:
                pass

        return ""

    @property
    def has_url(self) -> bool:
        """Check if a database URL is configured."""
        return bool(self.resolve_url())

    def get_pool_settings(self) -> Dict[str, Any]:
        """Get connection pool settings from config or defaults."""
        self._load_file_config()

        db_config = self._raw_config.get("database", {})
        pool_config = db_config.get("pool", {})

        return {
            "max_connections": pool_config.get("max_connections", self.max_connections),
            "min_connections": pool_config.get("min_connections", self.min_connections),
            "connect_timeout": pool_config.get("connect_timeout", self.connect_timeout),
            "idle_timeout": pool_config.get("idle_timeout", self.idle_timeout),
            "max_lifetime": pool_config.get("max_lifetime", self.max_lifetime),
        }


_config: Optional[Config] = None


def get_config() -> Config:
    """Get the global CLI config instance."""
    global _config
    if _config is None:
        _config = Config()
    return _config


def set_config(config: Config) -> None:
    """Set the global CLI config instance."""
    global _config
    _config = config


def reset_config() -> None:
    """Reset the global config (mainly for testing)."""
    global _config
    _config = None
