"""
Ryx CLI — color and formatting utilities.

Usage::

    from ryx.cli.style import PREFIX, OK, FAIL, WARN, green, cyan

    print(f"{PREFIX} {OK} Migration applied")
    print(f"{PREFIX} {FAIL} {red('Error:')} something broke")
"""

from __future__ import annotations

import os
import sys

_RESET = "\033[0m"
_BOLD = "\033[1m"
_RED = "\033[31m"
_GREEN = "\033[32m"
_YELLOW = "\033[33m"
_BLUE = "\033[34m"
_MAGENTA = "\033[35m"
_CYAN = "\033[36m"
_GREY = "\033[90m"


def _supports_color() -> bool:
    if not sys.stdout.isatty():
        return False
    if os.environ.get("NO_COLOR"):
        return False
    term = os.environ.get("TERM", "")
    if term == "dumb":
        return False
    return True


_USE_COLOR = _supports_color()


def _c(text: str, code: str) -> str:
    return f"{code}{text}{_RESET}" if _USE_COLOR else text


def bold(text: str) -> str:
    return _c(text, _BOLD)


def dim(text: str) -> str:
    return _c(text, _GREY)


def red(text: str) -> str:
    return _c(text, _RED)


def green(text: str) -> str:
    return _c(text, _GREEN)


def yellow(text: str) -> str:
    return _c(text, _YELLOW)


def blue(text: str) -> str:
    return _c(text, _BLUE)


def magenta(text: str) -> str:
    return _c(text, _MAGENTA)


def cyan(text: str) -> str:
    return _c(text, _CYAN)


PREFIX = _c("[ryx]", f"{_BLUE}{_BOLD}") if _USE_COLOR else "[ryx]"
OK = _c("✓", _GREEN) if _USE_COLOR else "✓"
FAIL = _c("✗", _RED) if _USE_COLOR else "✗"
WARN = _c("⚠", _YELLOW) if _USE_COLOR else "⚠"
