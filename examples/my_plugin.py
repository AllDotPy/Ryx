"""
Example CLI plugin for ryx.

This demonstrates how to create custom commands as plugins.

Usage:
    1. Add to your ryx_settings.py:
        CLI_PLUGINS = ["examples.my_plugin.MyCustomPlugin"]

    2. Or register via pyproject.toml entry points:
        [project.entry-points.ryx_cli_plugins]
        myplugin = "examples.my_plugin:MyCustomPlugin"
"""

from __future__ import annotations

import argparse

from ryx.cli.commands.base import Command
from ryx.cli.plugins import Plugin


class ShowUrlsCommand(Command):
    """Show all registered models and their table names."""

    name = "showurls"
    help = "Show all models and their table names"
    description = (
        "List all registered ryx models with their corresponding database tables"
    )

    def add_arguments(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "--verbose",
            "-v",
            action="store_true",
            help="Show additional field information",
        )

    async def execute(self, args: argparse.Namespace) -> int:
        from ryx.models import Model

        print("\nRegistered Models:")
        print("-" * 60)

        # Get all Model subclasses
        models = []
        for cls in Model.__subclasses__():
            if hasattr(cls, "_meta") and cls._meta.table_name:
                models.append(cls)

        if not models:
            print("No models registered.")
            return 0

        for model in sorted(models, key=lambda m: m._meta.table_name):
            print(f"  {model.__name__}: {model._meta.table_name}")

            if getattr(args, "verbose", False):
                for field_name, field in model._meta.fields.items():
                    pk = " [PK]" if field.primary_key else ""
                    print(f"    - {field_name}: {field.__class__.__name__}{pk}")

        print(f"\nTotal: {len(models)} model(s)")
        return 0


class MyCustomPlugin(Plugin):
    """Example plugin that adds custom commands."""

    name = "example_plugin"
    version = "1.0.0"
    description = "Example plugin showing custom command registration"

    def get_commands(self):
        return [ShowUrlsCommand]

    def on_load(self):
        print(f"[ryx] Loaded {self.name} v{self.version}")


# For direct registration
def get_plugin():
    return MyCustomPlugin()
