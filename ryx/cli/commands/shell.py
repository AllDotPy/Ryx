from __future__ import annotations

import argparse
import importlib
import sys

from ryx.cli.commands.base import Command
from ryx.cli.config import get_config


class ShellCommand(Command):
    """Start an interactive Python shell with ORM pre-loaded."""

    name = "shell"
    help = "Start interactive Python shell"
    description = (
        "Start an interactive Python shell with ryx ORM pre-loaded. "
        "Models can be automatically imported if specified."
    )

    def add_arguments(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "--models", metavar="MODULE", help="Pre-import models from this module"
        )
        parser.add_argument(
            "--query",
            "-q",
            metavar="QUERY",
            help="Execute a query and print results (non-interactive)",
        )
        parser.add_argument(
            "--ipython",
            action="store_true",
            help="Use IPython with full features (syntax highlighting, completions)",
        )
        parser.add_argument(
            "--notebook",
            action="store_true",
            help="Launch Jupyter notebook instead of shell",
        )

    async def execute(self, args: argparse.Namespace) -> int:
        config = get_config()
        url = self._resolve_url(args, config)

        banner = "ryx ORM interactive shell\n"

        if url:
            banner += f"Connected to: {self._mask_url(url)}\n"

        models_module = getattr(args, "models", None)
        if models_module:
            banner += f"Models loaded from: {models_module}\n"

        banner += "\nType 'exit()' or Ctrl-D to quit.\n"

        use_ipython = getattr(args, "ipython", False)

        if use_ipython:
            # Run IPython in a new process to completely avoid asyncio event loop issues
            self._run_ipython_subprocess(url, banner)
        else:
            import code

            code.interact(banner=banner, local={})

        return 0

    def _run_ipython_subprocess(self, url: str, banner: str) -> None:
        """Run IPython in a subprocess - completely avoids asyncio event loop issues."""
        import subprocess
        import os
        import sys

        code = f"""
import asyncio

# Set up asyncio policy
try:
    asyncio.set_event_loop_policy(asyncio.DefaultEventLoopPolicy())
except:
    pass

# Import and setup ryx
from ryx import setup
from ryx.queryset import run_sync

if {repr(url)}:
    run_sync(setup({repr(url)}))

# Setup IPython with full features
from IPython.terminal.interactiveshell import TerminalInteractiveShell

shell = TerminalInteractiveShell.instance(
    banner1={repr(banner)},
    colors="Linux",
)

# Make ryx available
import ryx
shell.user_ns["ryx"] = ryx

shell.interact()
"""

        try:
            subprocess.run(
                [sys.executable, "-c", code],
                env={k: v for k, v in os.environ.items() if k != "PYTHONPATH"},
            )
        except Exception as e:
            print(f"[WARNING] IPython failed: {e}", file=sys.stderr)

    async def _execute_query(self, query: str, ns: dict, banner: str) -> int:
        """Execute a query in non-interactive mode."""
        try:
            from ryx.queryset import run_sync

            result = run_sync(self._eval_query(query, ns))
            if result is not None:
                print(result)
            return 0
        except Exception as e:
            print(f"[ERROR] {type(e).__name__}: {e}", file=sys.stderr)
            return 1

    async def _eval_query(self, query: str, ns: dict):
        """Eval the query in the context of the shell namespace."""
        code = compile(query, "<query>", "eval")
        return eval(code, ns)

    def _resolve_url(self, args, config) -> str:
        url = getattr(args, "url", None)
        if url:
            return url
        return config.resolve_url()

    def _mask_url(self, url: str) -> str:
        import re

        return re.sub(r"(:)[^:@/]+(@)", r"\1***\2", url)


async def cmd_shell(args) -> None:
    cmd = ShellCommand()
    await cmd.execute(args)
