"""
ryx ORM — Command-Line Interface

Run with:  python -m ryx <command> [options]

Commands:
  migrate           Apply all pending migrations to the database
  makemigrations    Detect model changes and generate migration files
  showmigrations    List all migrations and their applied status
  sqlmigrate        Print the SQL for a specific migration (dry run)
  flush             Delete all rows from all model tables (DANGEROUS)
  shell             Start an interactive Python shell with ORM pre-loaded
  dbshell           Connect directly to the database (psql/mysql/sqlite3)
  version           Print ryx version
  inspectdb         Introspect an existing database and print model stubs

Configuration is read from (in order):
  1. CLI flags (--url, --settings)
  2. ryx_DATABASE_URL environment variable
  3. ryx_settings.py in the current directory

Usage examples:
  python -m ryx migrate --url postgres://user:pass@localhost/mydb
  python -m ryx makemigrations --models myapp.models --dir migrations/
  python -m ryx shell --url sqlite:///dev.db
  python -m ryx showmigrations
  python -m ryx version
"""

from __future__ import annotations

import argparse
import asyncio
import importlib
import os
import sys
from pathlib import Path
from typing import List, Optional


#
# Entry point
#
def main() -> None:
    """Main entry point for `python -m ryx`."""
    parser = _build_parser()
    args   = parser.parse_args()

    if not hasattr(args, "func"):
        parser.print_help()
        sys.exit(0)

    # Run async commands in an event loop
    try:
        asyncio.run(args.func(args))
    except KeyboardInterrupt:
        print("\nAborted.")
        sys.exit(1)
    except Exception as e:
        print(f"\n[ERROR] {type(e).__name__}: {e}", file=sys.stderr)
        sys.exit(1)


#
# Argument parser
#
def _build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(
        prog        = "python -m ryx",
        description = "ryx ORM — command-line management tool",
    )
    p.add_argument(
        "--url", "-u",
        metavar = "DATABASE_URL",
        help    = "Database URL (overrides ryx_DATABASE_URL env var)",
    )
    p.add_argument(
        "--settings", "-s",
        metavar = "MODULE",
        help    = "Python module with ryx settings (default: ryx_settings)",
    )

    sub = p.add_subparsers(title="commands", dest="command")

    # migrate
    m = sub.add_parser("migrate", help="Apply pending migrations")
    m.add_argument("--dry-run", action="store_true", help="Print SQL without executing")
    m.add_argument("--models",  metavar="MODULE", help="Dotted module path containing models")
    m.add_argument("--dir",     default="migrations", metavar="DIR", help="Migrations directory")
    m.set_defaults(func=cmd_migrate)

    # makemigrations
    mk = sub.add_parser("makemigrations", help="Detect changes and generate migration files")
    mk.add_argument("--models", metavar="MODULE", required=True, help="Dotted module path")
    mk.add_argument("--dir",    default="migrations", metavar="DIR")
    mk.add_argument("--name",   metavar="NAME", help="Override migration name slug")
    mk.add_argument("--check",  action="store_true", help="Exit 1 if changes detected (CI mode)")
    mk.set_defaults(func=cmd_makemigrations)

    # showmigrations
    sm = sub.add_parser("showmigrations", help="List migrations and their status")
    sm.add_argument("--dir", default="migrations", metavar="DIR")
    sm.set_defaults(func=cmd_showmigrations)

    # sqlmigrate
    sq = sub.add_parser("sqlmigrate", help="Print SQL for a migration (dry run)")
    sq.add_argument("name", help="Migration name (e.g. 0001_initial)")
    sq.add_argument("--dir", default="migrations", metavar="DIR")
    sq.set_defaults(func=cmd_sqlmigrate)

    # flush
    fl = sub.add_parser("flush", help="Delete all rows from all tables (DANGEROUS)")
    fl.add_argument("--models", metavar="MODULE", required=True)
    fl.add_argument("--yes", action="store_true", help="Skip confirmation prompt")
    fl.set_defaults(func=cmd_flush)

    # shell
    sh = sub.add_parser("shell", help="Interactive Python shell with ORM pre-loaded")
    sh.add_argument("--models", metavar="MODULE", help="Pre-import models from this module")
    sh.set_defaults(func=cmd_shell)

    # dbshell
    db = sub.add_parser("dbshell", help="Connect to the database via its CLI tool")
    db.set_defaults(func=cmd_dbshell)

    # version
    v = sub.add_parser("version", help="Print ryx version")
    v.set_defaults(func=cmd_version)

    # inspectdb
    ins = sub.add_parser("inspectdb", help="Print model stubs from an existing database")
    ins.add_argument("--table", metavar="TABLE", help="Inspect only this table")
    ins.set_defaults(func=cmd_inspectdb)

    return p


#
# Command implementations
#
async def cmd_version(args) -> None:
    """Print ryx version."""
    try:
        from ryx import __version__
        print(f"ryx ORM {__version__}")
    except Exception:
        print("ryx ORM (version unknown)")


async def cmd_migrate(args) -> None:
    """Apply all pending migrations."""
    url = _get_url(args)
    print(f"[ryx] Connecting to {_mask_url(url)} ...")

    import ryx
    await ryx.setup(url)

    models = _load_models(getattr(args, "models", None))
    from ryx.migrations import MigrationRunner

    runner = MigrationRunner(models, dry_run=getattr(args, "dry_run", False))
    changes = await runner.migrate()

    if changes:
        print(f"[ryx] Applied {len(changes)} change(s).")
    else:
        print("[ryx] No pending migrations.")


async def cmd_makemigrations(args) -> None:
    """Detect changes and generate migration files."""
    models = _load_models(args.models)
    if not models:
        print("[ryx] No models found. Pass --models myapp.models")
        sys.exit(1)

    from ryx.migrations.autodetect import Autodetector
    detector = Autodetector(models=models, migrations_dir=args.dir)
    operations = detector.detect()

    if not operations:
        print("[ryx] No changes detected.")
        if args.check:
            sys.exit(0)
        return

    if args.check:
        print(f"[ryx] {len(operations)} change(s) detected:")
        for op in operations:
            print(f"  - {op.describe()}")
        sys.exit(1)

    path = detector.write_migration(operations)
    print(f"[ryx] Created migration: {path}")
    for op in operations:
        print(f"  - {op.describe()}")


async def cmd_showmigrations(args) -> None:
    """List migrations and their applied/pending status."""
    mig_dir = Path(args.dir)
    if not mig_dir.exists():
        print(f"[ryx] No migrations directory found at: {mig_dir}")
        return

    files = sorted(mig_dir.glob("[0-9]*.py"))
    if not files:
        print("[ryx] No migrations found.")
        return

    # Try to check which are applied (requires DB connection)
    applied = set()
    url = _get_url(args, required=False)
    if url:
        try:
            import ryx
            await ryx.setup(url)
            from ryx.executor_helpers import raw_fetch
            rows = await raw_fetch(
                'SELECT name FROM "ryx_migrations"'
            )
            applied = {r.get("name", "") for r in rows}
        except Exception:
            pass

    print(f"\nMigrations in {mig_dir}:")
    for f in files:
        status = "✓ applied" if f.stem in applied else "  pending"
        print(f"  [{status}]  {f.stem}")
    print()


async def cmd_sqlmigrate(args) -> None:
    """Print the SQL statements for a migration without executing them."""
    mig_dir = Path(args.dir)
    mig_file = mig_dir / f"{args.name}.py"

    if not mig_file.exists():
        # Try with glob
        matches = list(mig_dir.glob(f"{args.name}*.py"))
        if not matches:
            print(f"[ryx] Migration not found: {args.name}")
            sys.exit(1)
        mig_file = matches[0]

    import importlib.util
    spec = importlib.util.spec_from_file_location(mig_file.stem, mig_file)
    mod  = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)

    from ryx.migrations.ddl import DDLGenerator
    gen = DDLGenerator()  # default postgres

    print(f"\n-- SQL for migration: {mig_file.name}\n")
    for op in mod.Migration.operations:
        from ryx.migrations.autodetect import CreateTable, AddField, AlterField, CreateIndex
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


async def cmd_flush(args) -> None:
    """Delete all rows from all model tables."""
    if not args.yes:
        ans = input(
            "⚠️  This will DELETE ALL ROWS from all tables. "
            "Type 'yes' to confirm: "
        )
        if ans.strip().lower() != "yes":
            print("Aborted.")
            return

    url    = _get_url(args)
    models = _load_models(args.models)

    import ryx
    await ryx.setup(url)

    from ryx.executor_helpers import raw_execute
    for model in models:
        if hasattr(model, "_meta"):
            table = model._meta.table_name
            print(f"[ryx] Flushing {table}...")
            await raw_execute(f'DELETE FROM "{table}"')

    print("[ryx] Flush complete.")


async def cmd_shell(args) -> None:
    """Start an interactive Python shell with ORM pre-loaded."""
    url = _get_url(args, required=False)
    banner = "ryx ORM interactive shell\n"

    ns: dict = {}

    if url:
        import ryx as _ryx
        await _ryx.setup(url)
        ns["ryx"] = _ryx
        banner += f"Connected to: {_mask_url(url)}\n"

    models_module = getattr(args, "models", None)
    if models_module:
        try:
            mod = importlib.import_module(models_module)
            ns.update({k: v for k, v in vars(mod).items() if not k.startswith("_")})
            banner += f"Models loaded from: {models_module}\n"
        except ImportError as e:
            banner += f"Warning: could not load models ({e})\n"

    banner += "\nType 'exit()' or Ctrl-D to quit.\n"

    try:
        import IPython
        IPython.start_ipython(argv=[], user_ns=ns, display_banner=False)
        print(banner)
    except ImportError:
        import code
        code.interact(banner=banner, local=ns)


async def cmd_dbshell(args) -> None:
    """Open the database's native CLI tool."""
    import subprocess
    url = _get_url(args)

    if url.startswith("postgres"):
        subprocess.run(["psql", url])
    elif url.startswith("mysql"):
        # Parse mysql://user:pass@host/db
        subprocess.run(["mysql", "--url", url])
    elif url.startswith("sqlite"):
        db_path = url.removeprefix("sqlite:///").removeprefix("sqlite://")
        subprocess.run(["sqlite3", db_path])
    else:
        print(f"[ryx] Don't know which CLI tool to use for: {url}")
        sys.exit(1)


async def cmd_inspectdb(args) -> None:
    """Introspect the database and print model class stubs."""
    url = _get_url(args)
    import ryx
    await ryx.setup(url)

    from ryx.executor_helpers import raw_fetch

    # Get table list (Postgres / MySQL)
    try:
        tables = await raw_fetch(
            "SELECT table_name FROM information_schema.tables "
            "WHERE table_schema = 'public' AND table_type = 'BASE TABLE'"
        )
    except Exception:
        tables = await raw_fetch(
            "SELECT name AS table_name FROM sqlite_master WHERE type='table'"
        )

    target_table = getattr(args, "table", None)

    print("# Generated by `python -m ryx inspectdb`\n")
    print("from ryx import Model, CharField, IntField, BooleanField, TextField\n")
    print("from ryx import DateTimeField, FloatField, DecimalField\n\n")

    for row in tables:
        table_name = row.get("table_name") or row.get("name", "")
        if not table_name or table_name.startswith("ryx_"):
            continue
        if target_table and table_name != target_table:
            continue

        # Fetch columns
        try:
            cols = await raw_fetch(
                f"SELECT column_name, data_type, is_nullable, column_default "
                f"FROM information_schema.columns WHERE table_name = '{table_name}' "
                f"ORDER BY ordinal_position"
            )
        except Exception:
            cols = await raw_fetch(f"PRAGMA table_info(\"{table_name}\")")

        class_name = _snake_to_pascal(table_name)
        print(f"class {class_name}(Model):")
        print("    class Meta:")
        print(f"        table_name = {table_name!r}\n")

        for col in cols:
            col_name = col.get("column_name") or col.get("name", "unknown")
            col_type = (col.get("data_type") or col.get("type") or "TEXT").upper()
            nullable = col.get("is_nullable", "YES") == "YES" or bool(col.get("notnull", 0) == 0)
            field_type = _db_type_to_field(col_type)
            null_kw = ", null=True" if nullable else ""
            print(f"    {col_name} = {field_type}({null_kw})")

        print()


#
# Helpers
#
def _get_url(args, required: bool = True) -> str:
    """Get the database URL from CLI arg, env var, or settings module."""
    url = getattr(args, "url", None) or os.environ.get("ryx_DATABASE_URL")

    if not url:
        # Try settings module
        settings_mod = getattr(args, "settings", None) or "ryx_settings"
        try:
            mod = importlib.import_module(settings_mod)
            url = getattr(mod, "DATABASE_URL", None)
        except ImportError:
            pass

    if not url and required:
        print(
            "[ryx] No database URL found.\n"
            "  Set ryx_DATABASE_URL environment variable, or\n"
            "  pass --url postgres://user:pass@host/db, or\n"
            "  create ryx_settings.py with DATABASE_URL = '...'"
        )
        sys.exit(1)

    return url or ""


def _load_models(models_module: Optional[str]) -> list:
    """Import all Model subclasses from a dotted module path."""
    if not models_module:
        return []
    try:
        mod = importlib.import_module(models_module)
    except ImportError as e:
        print(f"[ryx] Cannot import '{models_module}': {e}")
        sys.exit(1)

    from ryx.models import Model
    return [
        cls for cls in vars(mod).values()
        if isinstance(cls, type) and issubclass(cls, Model) and cls is not Model
    ]


def _mask_url(url: str) -> str:
    """Replace password in URL with *** for safe logging."""
    import re
    return re.sub(r"(:)[^:@/]+(@)", r"\1***\2", url)


def _snake_to_pascal(name: str) -> str:
    """Convert snake_case table name to PascalCase class name."""
    return "".join(w.capitalize() for w in name.split("_"))


def _db_type_to_field(db_type: str) -> str:
    """Map a SQL type string to a ryx field class name."""
    dt = db_type.upper()
    if "INT" in dt:         
        return "IntField"
    if "FLOAT" in dt or "REAL" in dt or "DOUBLE" in dt: 
        return "FloatField"
    if "NUMERIC" in dt or "DECIMAL" in dt: 
        return "DecimalField"
    if "BOOL" in dt:        
        return "BooleanField"
    if "TEXT" in dt:        
        return "TextField"
    if "TIMESTAMP" in dt or "DATETIME" in dt: 
        return "DateTimeField"
    return "CharField(max_length=255)"  # default


if __name__ == "__main__":
    main()