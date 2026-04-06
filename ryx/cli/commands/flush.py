from __future__ import annotations

import argparse
import sys

from ryx.cli.commands.base import Command
from ryx.cli.config import get_config


class FlushCommand(Command):
    """Delete all rows from all model tables."""

    name = "flush"
    help = "Delete all rows from all tables (DANGEROUS)"
    description = (
        "Delete all rows from all model tables. This is a destructive operation "
        "and should be used with caution."
    )

    def add_arguments(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "--models",
            metavar="MODULE",
            required=True,
            help="Dotted module path containing models",
        )
        parser.add_argument(
            "--yes", action="store_true", help="Skip confirmation prompt"
        )
        parser.add_argument(
            "--force",
            action="store_true",
            help="Force flush without any confirmation (alias for --yes)",
        )

    async def execute(self, args: argparse.Namespace) -> int:
        yes = getattr(args, "yes", False) or getattr(args, "force", False)

        if not yes:
            ans = input(
                "⚠️  This will DELETE ALL ROWS from all tables. Type 'yes' to confirm: "
            )
            if ans.strip().lower() != "yes":
                print("Aborted.")
                return 0

        config = get_config()
        url = self._resolve_url(args, config)

        if not url:
            self._print_missing_url()
            return 1

        import ryx

        await ryx.setup(url)

        models = self._load_models(args.models)

        from ryx.executor_helpers import raw_execute

        for model in models:
            if hasattr(model, "_meta"):
                table = model._meta.table_name
                print(f"[ryx] Flushing {table}...")
                await raw_execute(f'DELETE FROM "{table}"')

        print("[ryx] Flush complete.")
        return 0

    def _resolve_url(self, args, config) -> str:
        url = getattr(args, "url", None)
        if url:
            return url
        return config.resolve_url()

    def _load_models(self, models_module: str) -> list:
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

    def _print_missing_url(self) -> None:
        print(
            "[ryx] No database URL found.\n"
            "  Set RYX_DATABASE_URL environment variable, or\n"
            "  pass --url postgres://user:pass@host/db"
        )


# Legacy function for backward compatibility
async def cmd_flush(args) -> None:
    cmd = FlushCommand()
    await cmd.execute(args)
