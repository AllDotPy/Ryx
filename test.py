#!/usr/bin/env python3
"""
Test runner script for Ryx ORM.

This script provides convenient commands to run different test suites.
"""

import argparse
import subprocess
import sys
from pathlib import Path

def run_command(cmd, cwd=None):
    """Run a command and return the result."""
    try:
        result = subprocess.run(
            cmd,
            shell=True,
            cwd=cwd or Path(__file__).parent,
            capture_output=True,
            text=True,
            check=True
        )
        return result
    except subprocess.CalledProcessError as e:
        print(f"Command failed: {cmd}")
        print(f"STDOUT: {e.stdout}")
        print(f"STDERR: {e.stderr}")
        sys.exit(1)

def main():
    parser = argparse.ArgumentParser(description="Ryx ORM Test Runner")
    parser.add_argument(
        "command",
        choices=["unit", "integration", "all", "coverage", "check"],
        help="Test command to run"
    )
    parser.add_argument(
        "--verbose", "-v",
        action="store_true",
        help="Verbose output"
    )
    parser.add_argument(
        "--no-cov",
        action="store_true",
        help="Skip coverage for coverage command"
    )

    args = parser.parse_args()

    # Ensure we're in development mode
    print("Ensuring Rust extension is built...")
    run_command("maturin develop")

    base_cmd = "python -m pytest"
    if args.verbose:
        base_cmd += " -v"

    if args.command == "unit":
        print("Running unit tests...")
        cmd = f"{base_cmd} tests/unit/"
        run_command(cmd)

    elif args.command == "integration":
        print("Running integration tests...")
        cmd = f"{base_cmd} tests/integration/"
        run_command(cmd)

    elif args.command == "all":
        print("Running all tests...")
        cmd = f"{base_cmd} tests/"
        run_command(cmd)

    elif args.command == "coverage":
        print("Running tests with coverage...")
        if args.no_cov:
            cmd = f"{base_cmd} tests/"
        else:
            cmd = f"{base_cmd} --cov=ryx --cov-report=html --cov-report=term tests/"
        run_command(cmd)
        if not args.no_cov:
            print("Coverage report generated in htmlcov/index.html")

    elif args.command == "check":
        print("Running code quality checks...")
        # Run tests with coverage
        run_command(f"{base_cmd} --cov=ryx --cov-report=term-missing tests/")

        # Check for unused imports, etc. (if tools are available)
        try:
            run_command("python -m flake8 ryx/ tests/ --max-line-length=100")
        except FileNotFoundError:
            print("flake8 not installed, skipping style checks")

    print("✓ All tests passed!")

if __name__ == "__main__":
    main()