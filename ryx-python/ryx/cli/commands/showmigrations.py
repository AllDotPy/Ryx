from __future__ import annotations

import argparse
from pathlib import Path

from ryx.cli.commands.base import Command
from ryx.cli.config import get_config
from ryx.cli.config_context import resolve_config
from ryx.cli.style import PREFIX, OK, WARN, cyan, green, yellow, red


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
            print(f"{PREFIX} {red('No migrations directory found at:')} {cyan(str(mig_dir))}")
            return 1

        files = sorted(mig_dir.rglob("[0-9]*.py"))
        if not files:
            print(f"{PREFIX} {WARN} No migrations found.")
            return 0

        applied = set()
        cfg = getattr(args, "resolved_config", None) or resolve_config(args)
        urls = cfg.urls
        url = urls.get(getattr(args, "db", None) or cfg.db_alias, urls.get("default")) if urls else None

        if url:
            try:
                import ryx

                await ryx.setup(url)
                from ryx.executor_helpers import raw_fetch

                rows = await raw_fetch('SELECT name FROM "ryx_migrations"')
                applied = {r.get("name", "") for r in rows}
            except Exception:
                pass

        print(f"\n{PREFIX} Migrations in {cyan(str(mig_dir))}:")
        for f in files:
            is_applied = f.stem in applied or any(entry.endswith(f"|{f.stem}") for entry in applied)
            status = f"{OK} {green(f.stem)}" if is_applied else f"  {yellow(f.stem)}"
            if getattr(args, "unapplied", False) and is_applied:
                continue
            print(f"  [{status}]")
        print()

        return 0


# Legacy function for backward compatibility
async def cmd_showmigrations(args) -> None:
    cmd = ShowMigrationsCommand()
    await cmd.execute(args)
