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

Configuration is read from (in order of precedence):
  1. CLI flags (--url, --settings, --config, --env)
  2. Config file (ryx.yaml/yml/toml if --config specified or in current dir)
  3. RYX_DATABASE_URL environment variable
  4. ryx_settings.py in the current directory

Usage examples:
  python -m ryx migrate --url postgres://user:pass@localhost/mydb
  python -m ryx makemigrations --models myapp.models --dir migrations/
  python -m ryx shell --url sqlite:///dev.db
  python -m ryx showmigrations
  python -m ryx version
  python -m ryx --config ryx.toml --env prod migrate
"""

from __future__ import annotations

import argparse
import asyncio
import sys


def main() -> None:
    """Main entry point for `python -m ryx`."""
    parser = _build_parser()
    args = parser.parse_args()

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


def _build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(
        prog="python -m ryx",
        description="ryx ORM — command-line management tool",
    )
    p.add_argument(
        "--url",
        "-u",
        metavar="DATABASE_URL",
        help="Database URL (overrides RYX_DATABASE_URL env var)",
    )
    p.add_argument(
        "--settings",
        "-s",
        metavar="MODULE",
        help="Python module with ryx settings (default: ryx_settings)",
    )
    p.add_argument(
        "--config",
        "-c",
        metavar="FILE",
        help="Path to config file (ryx.yaml, ryx.yml, ryx.toml)",
    )
    p.add_argument(
        "--env",
        metavar="ENV",
        choices=["dev", "test", "prod"],
        help="Environment name for multi-env config (dev/test/prod)",
    )

    sub = p.add_subparsers(title="commands", dest="command")

    # migrate
    m = sub.add_parser("migrate", help="Apply pending migrations")
    m.add_argument("--dry-run", action="store_true", help="Print SQL without executing")
    m.add_argument(
        "--models", metavar="MODULE", help="Dotted module path containing models"
    )
    m.add_argument(
        "--dir", default="migrations", metavar="DIR", help="Migrations directory"
    )
    m.add_argument(
        "--plan", action="store_true", help="Show migration plan without executing"
    )
    m.set_defaults(func=cmd_migrate)

    # makemigrations
    mk = sub.add_parser(
        "makemigrations", help="Detect changes and generate migration files"
    )
    mk.add_argument(
        "--models", metavar="MODULE", required=True, help="Dotted module path"
    )
    mk.add_argument("--dir", default="migrations", metavar="DIR")
    mk.add_argument("--name", metavar="NAME", help="Override migration name slug")
    mk.add_argument(
        "--check", action="store_true", help="Exit 1 if changes detected (CI mode)"
    )
    mk.add_argument(
        "--squash", action="store_true", help="Squash multiple migrations into one"
    )
    mk.set_defaults(func=cmd_makemigrations)

    # showmigrations
    sm = sub.add_parser("showmigrations", help="List migrations and their status")
    sm.add_argument("--dir", default="migrations", metavar="DIR")
    sm.add_argument(
        "--unapplied", action="store_true", help="Show only unapplied migrations"
    )
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
    fl.add_argument(
        "--force", action="store_true", help="Force flush without any confirmation"
    )
    fl.set_defaults(func=cmd_flush)

    # shell
    sh = sub.add_parser("shell", help="Start an interactive Python shell")
    sh.add_argument(
        "--models", metavar="MODULE", help="Pre-import models from this module"
    )
    sh.add_argument(
        "--query",
        "-q",
        metavar="QUERY",
        help="Execute query and exit (non-interactive)",
    )
    sh.add_argument("--notebook", action="store_true", help="Launch Jupyter notebook")
    sh.set_defaults(func=cmd_shell)

    # dbshell
    db = sub.add_parser("dbshell", help="Connect to the database via its CLI tool")
    db.add_argument("--command", "-c", metavar="CMD", help="Execute command and exit")
    db.set_defaults(func=cmd_dbshell)

    # version
    v = sub.add_parser("version", help="Print ryx version")
    v.add_argument(
        "--verbose", "-v", action="store_true", help="Show additional version info"
    )
    v.set_defaults(func=cmd_version)

    # inspectdb
    ins = sub.add_parser(
        "inspectdb", help="Print model stubs from an existing database"
    )
    ins.add_argument("--table", metavar="TABLE", help="Inspect only this table")
    ins.add_argument("--output", "-o", metavar="FILE", help="Write output to file")
    ins.set_defaults(func=cmd_inspectdb)

    return p


#
# Command implementations (delegating to new CLI module for future extensibility)
#


async def cmd_version(args) -> None:
    from ryx.cli.commands.version import cmd_version as new_cmd

    await new_cmd(args)


async def cmd_migrate(args) -> None:
    from ryx.cli.commands.migrate import cmd_migrate as new_cmd

    await new_cmd(args)


async def cmd_makemigrations(args) -> None:
    from ryx.cli.commands.makemigrations import cmd_makemigrations as new_cmd

    await new_cmd(args)


async def cmd_showmigrations(args) -> None:
    from ryx.cli.commands.showmigrations import cmd_showmigrations as new_cmd

    await new_cmd(args)


async def cmd_sqlmigrate(args) -> None:
    from ryx.cli.commands.sqlmigrate import cmd_sqlmigrate as new_cmd

    await new_cmd(args)


async def cmd_flush(args) -> None:
    from ryx.cli.commands.flush import cmd_flush as new_cmd

    await new_cmd(args)


async def cmd_shell(args) -> None:
    from ryx.cli.commands.shell import cmd_shell as new_cmd

    await new_cmd(args)


async def cmd_dbshell(args) -> None:
    from ryx.cli.commands.dbshell import cmd_dbshell as new_cmd

    await new_cmd(args)


async def cmd_inspectdb(args) -> None:
    from ryx.cli.commands.inspectdb import cmd_inspectdb as new_cmd

    await new_cmd(args)


if __name__ == "__main__":
    main()
