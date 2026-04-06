from __future__ import annotations

import argparse
import sys

from ryx.cli.commands.base import Command


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
            required=True,
            help="Dotted module path containing models",
        )
        parser.add_argument(
            "--dir",
            default="migrations",
            metavar="DIR",
            help="Migrations directory (default: migrations)",
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
        models = self._load_models(args.models)
        if not models:
            print("[ryx] No models found. Pass --models myapp.models")
            return 1

        from ryx.migrations.autodetect import Autodetector

        detector = Autodetector(models=models, migrations_dir=args.dir)
        operations = detector.detect()

        if not operations:
            print("[ryx] No changes detected.")
            if args.check:
                return 0
            return 0

        if args.check:
            print(f"[ryx] {len(operations)} change(s) detected:")
            for op in operations:
                print(f"  - {op.describe()}")
            return 1

        path = detector.write_migration(operations)
        print(f"[ryx] Created migration: {path}")
        for op in operations:
            print(f"  - {op.describe()}")

        return 0

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


# Legacy function for backward compatibility
async def cmd_makemigrations(args) -> None:
    cmd = MakeMigrationsCommand()
    await cmd.execute(args)
