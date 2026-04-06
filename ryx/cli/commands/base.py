from __future__ import annotations

import argparse
from abc import ABC, abstractmethod
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from ryx.cli.config import Config


class Command(ABC):
    """Abstract base class for CLI commands."""

    name: str = ""
    help: str = ""
    description: str = ""

    def __init__(self, config: "Config | None" = None):
        self.config = config

    @abstractmethod
    async def execute(self, args: argparse.Namespace) -> int:
        """Execute the command.

        Returns:
            Exit code (0 for success, non-zero for failure).
        """

    def add_arguments(self, parser: argparse.ArgumentParser) -> None:
        """Add command-specific arguments to the parser.

        Override this method in subclasses to add custom arguments.
        """

    def configure(self, config: "Config") -> None:
        """Configure the command with global settings."""
        self.config = config
