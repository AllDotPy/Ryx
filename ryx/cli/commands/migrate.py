from __future__ import annotations

import argparse
import asyncio
import sys
from pathlib import Path
from typing import List, Optional

from ryx.cli.commands.base import Command
from ryx.cli.config import get_config, Config


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

    async def execute(self, args: argparse.Namespace) -> int:
        config = get_config()
        url = self._resolve_url(args, config)

        if not url:
            self._print_missing_url()
            return 1

        print(f"[ryx] Connecting to {self._mask_url(url)} ...")

        import ryx

        await ryx.setup(url)

        models = self._load_models(getattr(args, "models", None))
        from ryx.migrations import MigrationRunner

        runner = MigrationRunner(models, dry_run=getattr(args, "dry_run", False))

        if getattr(args, "plan", False):
            changes = runner.migrate()  # This is async
            # For plan, we'd need to run it but not apply
            # For now, fall through to normal migrate
            print("[ryx] --plan not yet implemented, running migrate...")

        changes = await runner.migrate()

        if changes:
            print(f"[ryx] Applied {len(changes)} change(s).")
        else:
            print("[ryx] No pending migrations.")

        return 0

    def _resolve_url(self, args, config: Config) -> str:
        url = getattr(args, "url", None)
        if url:
            return url
        return config.resolve_url()

    def _load_models(self, models_module: Optional[str]) -> list:
        if not models_module:
            return []
        try:
            import importlib

            mod = importlib.import_module(models_module)
        except ImportError as e:
            print(f"[ryx] Cannot import '{models_module}': {e}")
            sys.exit(1)

        from ryx.models import Model

        return [
            cls
            for cls in vars(mod).values()
            if isinstance(cls, type) and issubclass(cls, Model) and cls is not Model
        ]

    def _mask_url(self, url: str) -> str:
        import re

        return re.sub(r"(:)[^:@/]+(@)", r"\1***\2", url)

    def _print_missing_url(self) -> None:
        print(
            "[ryx] No database URL found.\n"
            "  Set RYX_DATABASE_URL environment variable, or\n"
            "  pass --url postgres://user:pass@host/db, or\n"
            "  create ryx_settings.py with DATABASE_URL = '...'"
        )


# Legacy function for backward compatibility
async def cmd_migrate(args) -> None:
    cmd = MigrateCommand()
    await cmd.execute(args)
