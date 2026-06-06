from __future__ import annotations

import argparse
import sys
from typing import Optional

from ryx.cli.commands.base import Command
from ryx.cli.config import get_config, Config
from ryx.cli.style import PREFIX, OK, FAIL, cyan, green


class MigrateCommand(Command):
    """Apply pending migrations to the database."""

    name = "migrate"
    help = "Apply pending migrations"
    description = "Apply all pending migrations to the database"

    def add_arguments(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "--dry-run", action="store_true", help="Print SQL without executing"
        )
        parser.add_argument(
            "--models", metavar="MODULE", help="Dotted module path containing models"
        )
        parser.add_argument(
            "--dir",
            default="migrations",
            metavar="DIR",
            help="Migrations directory (default: migrations)",
        )
        parser.add_argument(
            "--plan", action="store_true", help="Show migration plan without executing"
        )
        parser.add_argument(
            "--database",
            metavar="ALIAS",
            help="Run migrations for a specific database alias",
        )
        parser.add_argument(
            "--no-interactive",
            action="store_true",
            help="Fail if no migration files found (for CI)",
        )
        parser.add_argument(
            "--schema", metavar="SCHEMA", help="Database schema (PostgreSQL multi-schema)",
        )

    async def execute(self, args: argparse.Namespace) -> int:
        cfg = getattr(args, "resolved_config", None)
        urls = cfg.urls if cfg else None
        if not urls:
            config = get_config()
            urls = self._resolve_urls(args, config)

        if not urls:
            self._print_missing_url()
            return 1

        # Masking the first URL for the log
        first_url = list(urls.values())[0] if isinstance(urls, dict) else urls
        print(f"{PREFIX} Connecting to {cyan(self._mask_url(first_url))} ...")

        import ryx

        # Use the dictionary of URLs for multi-db setup
        await ryx.setup(urls)

        models = self._load_models(getattr(args, "models", None) or (cfg.models if cfg else None))
        from ryx.migrations import MigrationRunner

        runner = MigrationRunner(
            models,
            dry_run=getattr(args, "dry_run", False),
            alias_filter=getattr(args, "database", None) or (cfg.db_alias if cfg else None),
            schema=getattr(args, "schema", "") or "",
            migrations_dir=getattr(args, "dir", "migrations"),
            no_interactive=getattr(args, "no_interactive", False),
        )

        changes = await runner.migrate()

        if changes:
            count = green(str(len(changes)))
            print(f"{PREFIX} {OK} Applied {count} change(s)")
        else:
            print(f"{PREFIX} No pending migrations")

        return 0

    def _resolve_urls(self, args, config: Config) -> str | dict:
        url = getattr(args, "url", None)
        if url:
            return {"default": url}

        resolved = config.resolve_url()
        if resolved:
            # If resolve_url returns a string, wrap it
            if isinstance(resolved, str):
                return {"default": resolved}
            return resolved
        return None

    def _load_models(self, models_module: Optional[str | list]) -> list:
        if not models_module:
            return []
        modules = models_module if isinstance(models_module, list) else [models_module]
        collected = []
        from ryx.models import Model
        import importlib

        for mod_name in modules:
            try:
                mod = importlib.import_module(mod_name)
            except ImportError as e:
                print(f"[ryx] Cannot import '{mod_name}': {e}")
                sys.exit(1)
            collected.extend(
                [
                    cls
                    for cls in vars(mod).values()
                    if isinstance(cls, type)
                    and issubclass(cls, Model)
                    and cls is not Model
                ]
            )
        return collected

    def _mask_url(self, url: str) -> str:
        import re

        return re.sub(r"(:)[^:@/]+(@)", r"\1***\2", url)

    def _print_missing_url(self) -> None:
        from ryx.cli.style import FAIL, yellow, cyan
        print(
            f"{PREFIX} {FAIL} No database URL found.\n"
            f"  Set {cyan('RYX_DATABASE_URL')} environment variable, or\n"
            f"  pass {yellow('--url postgres://user:pass@host/db')}, or\n"
            f"  create {cyan('ryx_settings.py')} with {cyan('DATABASE_URL')} = '...'"
        )


# Legacy function for backward compatibility
async def cmd_migrate(args) -> None:
    cmd = MigrateCommand()
    await cmd.execute(args)
