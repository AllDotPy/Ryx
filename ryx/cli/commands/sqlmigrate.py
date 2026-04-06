from __future__ import annotations

import argparse
import importlib
import importlib.util
import sys
from pathlib import Path

from ryx.cli.commands.base import Command


class SqlMigrateCommand(Command):
    """Print SQL for a migration without executing it."""

    name = "sqlmigrate"
    help = "Print SQL for a migration (dry run)"
    description = "Generate and print the SQL for a migration without executing it"

    def add_arguments(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument("name", help="Migration name (e.g. 0001_initial)")
        parser.add_argument(
            "--dir",
            default="migrations",
            metavar="DIR",
            help="Migrations directory (default: migrations)",
        )
        parser.add_argument(
            "--backends",
            help="Filter to specific backends (comma-separated: postgres,mysql,sqlite)",
        )

    async def execute(self, args: argparse.Namespace) -> int:
        mig_dir = Path(args.dir)
        mig_file = mig_dir / f"{args.name}.py"

        if not mig_file.exists():
            # Try with glob
            matches = list(mig_dir.glob(f"{args.name}*.py"))
            if not matches:
                print(f"[ryx] Migration not found: {args.name}")
                return 1
            mig_file = matches[0]

        spec = importlib.util.spec_from_file_location(mig_file.stem, mig_file)
        mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(mod)

        from ryx.migrations.ddl import DDLGenerator

        gen = DDLGenerator()  # default postgres

        print(f"\n-- SQL for migration: {mig_file.name}\n")

        # Handle both new-style Migration class and old-style
        migration_ops = getattr(mod, "Migration", None)
        if migration_ops is None:
            migration_ops = getattr(mod, "operations", [])

        if hasattr(migration_ops, "operations"):
            ops = migration_ops.operations
        else:
            ops = migration_ops

        for op in ops:
            from ryx.migrations.autodetect import (
                CreateTable,
                AddField,
                AlterField,
                CreateIndex,
            )
            from ryx.migrations.state import TableState

            if isinstance(op, CreateTable):
                t = TableState(name=op.table)
                for col in op.columns:
                    t.add_column(col)
                print(gen.create_table(t) + ";\n")
            elif isinstance(op, AddField):
                print(gen.add_column(op.table, op.column) + ";\n")
            elif isinstance(op, AlterField):
                sql = gen.alter_column(op.table, op.new_col)
                if sql:
                    print(sql + ";\n")
            elif isinstance(op, CreateIndex):
                from ryx.models import Index

                idx = Index(fields=op.fields, name=op.name, unique=op.unique)
                print(gen.create_index(op.table, idx) + ";\n")

        return 0


# Legacy function for backward compatibility
async def cmd_sqlmigrate(args) -> None:
    cmd = SqlMigrateCommand()
    await cmd.execute(args)
