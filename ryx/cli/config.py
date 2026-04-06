from __future__ import annotations

import os
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional


@dataclass
class Config:
    """Global CLI configuration."""

    url: Optional[str] = None
    settings: str = "ryx_settings"
    debug: bool = False

    # Pool settings (passed to ryx.setup)
    max_connections: int = 10
    min_connections: int = 1
    connect_timeout: int = 30
    idle_timeout: int = 600
    max_lifetime: int = 1800

    @classmethod
    def from_args(cls, args) -> "Config":
        """Create config from parsed argparse.Namespace."""
        config = cls()
        config.url = getattr(args, "url", None)
        config.settings = getattr(args, "settings", "ryx_settings")
        return config

    def resolve_url(self) -> str:
        """Resolve database URL from CLI args, env var, or settings module."""
        # CLI arg takes precedence
        if self.url:
            return self.url

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
