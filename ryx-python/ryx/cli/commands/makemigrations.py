from __future__ import annotations

import argparse
import sys

from ryx.cli.commands.base import Command
from ryx.cli.style import PREFIX, OK, WARN, green, red, yellow


class MakeMigrationsCommand(Command):
    """Detect model changes and generate migration files."""

    name = "makemigrations"
    help = "Detect changes and generate migration files"
    description = (
        "Detect changes in your models and generate migration files. "
        "This compares the current state of your models against existing migrations."
    )

    def add_arguments(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "--models",
            metavar="MODULE",
            help="Dotted module path containing models (or use ryx.toml [models].files)",
        )
        parser.add_argument(
            "--dir",
            default="migrations",
            metavar="DIR",
            help="Migrations directory (default: migrations)",
        )
        parser.add_argument(
            "--alias",
            metavar="ALIAS",
            help="Database alias to generate migrations for (creates subdirectory)",
        )
        parser.add_argument(
            "--name", metavar="NAME", help="Override migration name slug"
        )
        parser.add_argument(
            "--check", action="store_true", help="Exit 1 if changes detected (CI mode)"
        )
        parser.add_argument(
            "--squash", action="store_true", help="Squash multiple migrations into one"
        )

    async def execute(self, args: argparse.Namespace) -> int:
        from ryx.cli.config_context import resolve_config

        cfg = getattr(args, "resolved_config", None) or resolve_config(args)
        models = self._load_models(args.models or cfg.models)
        if not models:
            print(f"{PREFIX} {WARN} No models found. Pass {yellow('--models myapp.models')} or set [models].files in ryx.toml")
            return 1

        from ryx.migrations.autodetect import Autodetector

        detector = Autodetector(
            models=models,
            migrations_dir=args.dir,
            alias=getattr(args, "alias", None),
        )
        operations = detector.detect()

        if not operations:
            print(f"{PREFIX} No changes detected.")
            if args.check:
                return 0
            return 0

        if args.check:
            print(f"{PREFIX} {red(str(len(operations)))} change(s) detected:")
            for op in operations:
                print(f"      {op.describe()}")
            return 1

        path = detector.write_migration(operations)
        print(f"{PREFIX} {OK} Created {green(str(path.name))}")
        for op in operations:
            print(f"      {op.describe()}")

        return 0

    def _load_models(self, models_module: str | list | None) -> list:
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


# Legacy function for backward compatibility
async def cmd_makemigrations(args) -> None:
    cmd = MakeMigrationsCommand()
    await cmd.execute(args)
