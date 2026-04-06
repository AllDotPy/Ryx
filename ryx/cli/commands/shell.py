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
            help="Use IPython if available (default: use standard Python shell)",
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
        ns: dict = {}

        if url:
            import ryx as _ryx
            from ryx.queryset import run_sync

            run_sync(_ryx.setup(url))
            ns["ryx"] = _ryx
            banner += f"Connected to: {self._mask_url(url)}\n"

        models_module = getattr(args, "models", None)
        if models_module:
            try:
                mod = importlib.import_module(models_module)
                ns.update({k: v for k, v in vars(mod).items() if not k.startswith("_")})
                banner += f"Models loaded from: {models_module}\n"
            except ImportError as e:
                banner += f"Warning: could not load models ({e})\n"

        # Handle query mode (non-interactive)
        if getattr(args, "query", None):
            return await self._execute_query(args.query, ns, banner)

        banner += "\nType 'exit()' or Ctrl-D to quit.\n"

        # Use IPython only if explicitly requested
        use_ipython = getattr(args, "ipython", False)

        if use_ipython:
            started = self._start_ipython(ns, banner)
            if not started:
                import code

                code.interact(banner=banner, local=ns)
        else:
            # Use standard Python shell
            import code

            code.interact(banner=banner, local=ns)

        return 0

    def _start_ipython(self, ns: dict, banner: str) -> bool:
        """Start IPython shell using a method that avoids event loop conflicts."""
        try:
            from IPython.terminal.interactiveshell import TerminalInteractiveShell

            shell = TerminalInteractiveShell.instance()
            shell.user_ns.update(ns)
            shell.banner1 = banner
            shell.interact()
            return True
        except ImportError:
            return False
        except Exception:
            return False

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
