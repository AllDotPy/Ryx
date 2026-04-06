from __future__ import annotations

import argparse
from pathlib import Path

from ryx.cli.commands.base import Command
from ryx.cli.config import get_config


class ShowMigrationsCommand(Command):
    """List all migrations and their applied status."""

    name = "showmigrations"
    help = "List migrations and their status"
    description = "List all migrations and show whether they have been applied"

    def add_arguments(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "--dir",
            default="migrations",
            metavar="DIR",
            help="Migrations directory (default: migrations)",
        )
        parser.add_argument(
            "--unapplied", action="store_true", help="Show only unapplied migrations"
        )

    async def execute(self, args: argparse.Namespace) -> int:
        mig_dir = Path(args.dir)
        if not mig_dir.exists():
            print(f"[ryx] No migrations directory found at: {mig_dir}")
            return 1

        files = sorted(mig_dir.glob("[0-9]*.py"))
        if not files:
            print("[ryx] No migrations found.")
            return 0

        # Try to check which are applied (requires DB connection)
        applied = set()
        config = get_config()
        url = config.resolve_url()

        if url:
            try:
                import ryx

                await ryx.setup(url)
                from ryx.executor_helpers import raw_fetch

                rows = await raw_fetch('SELECT name FROM "ryx_migrations"')
                applied = {r.get("name", "") for r in rows}
            except Exception:
                pass

        print(f"\nMigrations in {mig_dir}:")
        for f in files:
            status = "✓ applied" if f.stem in applied else "  pending"
            if getattr(args, "unapplied", False) and f.stem in applied:
                continue
            print(f"  [{status}]  {f.stem}")
        print()

        return 0


# Legacy function for backward compatibility
async def cmd_showmigrations(args) -> None:
    cmd = ShowMigrationsCommand()
    await cmd.execute(args)
