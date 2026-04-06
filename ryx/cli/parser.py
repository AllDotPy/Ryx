from __future__ import annotations

import argparse
from typing import Dict

from ryx.cli.commands.base import Command
from ryx.cli.registry import get_commands


def build_parser() -> argparse.ArgumentParser:
    """Build the main argument parser with all commands."""
    p = argparse.ArgumentParser(
        prog="python -m ryx",
        description="ryx ORM — command-line management tool",
    )

    # Global options
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
        "--verbose",
        "-v",
        action="store_true",
        help="Enable verbose output",
    )
    p.add_argument(
        "--debug",
        action="store_true",
        help="Enable debug mode",
    )

    sub = p.add_subparsers(title="commands", dest="command")

    # Register all commands
    commands = get_commands()
    for name, cmd_cls in commands.items():
        _add_command(sub, cmd_cls)

    return p


def _add_command(subparsers, cmd_cls: type) -> None:
    """Add a single command to the subparsers."""
    cmd = cmd_cls()
    parser = subparsers.add_parser(
        cmd.name,
        help=cmd.help,
        description=cmd.description,
    )
    cmd.add_arguments(parser)
    parser.set_defaults(func=lambda args: cmd_cls().execute(args))


class Parser:
    """Wrapper around ArgumentParser with command discovery."""

    def __init__(self):
        self._parser = None
        self._commands: Dict[str, Command] = {}

    @property
    def parser(self) -> argparse.ArgumentParser:
        if self._parser is None:
            self._parser = build_parser()
        return self._parser

    def parse_args(self, args=None):
        return self.parser.parse_args(args)

    def print_help(self, file=None):
        self.parser.print_help(file)


# Singleton instance
_parser = None


def get_parser() -> Parser:
    global _parser
    if _parser is None:
        _parser = Parser()
    return _parser
