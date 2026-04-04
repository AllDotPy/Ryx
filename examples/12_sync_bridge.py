"""
Ryx ORM — Example 12: Sync/Async Bridge

This example covers:
  - run_sync() — execute async ORM operations from synchronous code
  - sync_to_async() — wrap sync functions for async contexts
  - async_to_sync() — wrap async queries for synchronous callers
  - run_async() — run sync functions in a thread pool from async code
  - Practical patterns: WSGI integration, CLI scripts, mixed codebases
  - Thread safety considerations

Run with:
    uv run python examples/12_sync_bridge.py
"""

import asyncio
import os
import time
import threading
from pathlib import Path

import ryx
from ryx import Model, CharField, IntField
from ryx.queryset import run_sync, sync_to_async, async_to_sync, run_async
from ryx.migrations import MigrationRunner


DB_PATH = Path(__file__).parent.parent / "ryx_examples.sqlite3"
DATABASE_URL = f"sqlite://{DB_PATH}?mode=rwc"
os.environ["RYX_DATABASE_URL"] = DATABASE_URL


#
#  MODELS
#
class Task(Model):
    class Meta:
        table_name = "ex12_tasks"

    title = CharField(max_length=200)
    priority = IntField(default=0)
    is_done = False  # Not a field — just a Python attribute for demo


async def setup() -> None:
    await ryx.setup(DATABASE_URL)
    runner = MigrationRunner([Task])
    await runner.migrate()

    # Clean and seed
    await Task.objects.bulk_delete()
    tasks = [
        Task(title="Setup project", priority=1),
        Task(title="Write tests", priority=2),
        Task(title="Deploy to production", priority=3),
        Task(title="Write documentation", priority=1),
        Task(title="Code review", priority=2),
    ]
    await Task.objects.bulk_create(tasks)


#
#  RUN_SYNC — Async → Sync bridge
#
def sync_get_all_tasks() -> list:
    """Synchronous function that queries the async ORM.

    This is how you'd use Ryx from a WSGI app, a CLI script,
    or any synchronous codebase.
    """
    # run_sync() creates an event loop if needed and awaits the QuerySet
    tasks = run_sync(Task.objects.order_by("priority"))
    return tasks


def sync_get_task_count() -> int:
    """Sync function that calls an async method."""
    return run_sync(Task.objects.count())


async def demo_run_sync() -> None:
    print("\n" + "=" * 60)
    print("run_sync() — Async ORM from Sync Code")
    print("=" * 60)

    # Call sync functions that use the async ORM internally
    print("Calling sync_get_all_tasks() from async context:")
    # We need to run this in a thread since we're already in an event loop
    loop = asyncio.get_event_loop()
    tasks = await loop.run_in_executor(None, sync_get_all_tasks)
    print(f"  Found {len(tasks)} tasks:")
    for t in tasks:
        print(f"    [{t.priority}] {t.title}")

    count = await loop.run_in_executor(None, sync_get_task_count)
    print(f"\nTotal tasks (via sync): {count}")


#
#  SYNC_TO_ASYNC — Sync → Async bridge
#
def blocking_computation(n: int) -> int:
    """A CPU-bound or blocking synchronous function."""
    time.sleep(0.1)  # Simulate blocking I/O
    return n * n


async def demo_sync_to_async() -> None:
    print("\n" + "=" * 60)
    print("sync_to_async() — Wrap Sync Functions for Async")
    print("=" * 60)

    # Wrap the blocking function
    async_compute = sync_to_async(blocking_computation)

    # Now we can await it without blocking the event loop
    print("Running blocking computation in thread pool:")
    results = await asyncio.gather(
        async_compute(10),
        async_compute(20),
        async_compute(30),
    )
    print(f"  Results: {results}")

    # Run multiple blocking operations concurrently
    print("\nRunning 5 blocking ops concurrently:")
    start = time.monotonic()
    results = await asyncio.gather(
        *[sync_to_async(blocking_computation)(i) for i in range(5)]
    )
    elapsed = time.monotonic() - start
    print(f"  Results: {results}")
    print(f"  Time: {elapsed:.3f}s (should be ~0.1s, not 0.5s)")


#
#  ASYNC_TO_SYNC — Async → Sync bridge
#
async def get_high_priority_tasks() -> list:
    """Async function that queries the ORM."""
    return await Task.objects.filter(priority__gte=2).order_by("priority")


async def demo_async_to_sync() -> None:
    print("\n" + "=" * 60)
    print("async_to_sync() — Wrap Async Functions for Sync Code")
    print("=" * 60)

    # Wrap the async function so it can be called from sync code
    sync_get_high_priority = async_to_sync(get_high_priority_tasks)

    # In a real WSGI app, you'd call this directly:
    # tasks = sync_get_high_priority()

    # Since we're in an async context, we'll demonstrate via a thread
    loop = asyncio.get_event_loop()
    tasks = await loop.run_in_executor(None, sync_get_high_priority)
    print("High priority tasks (via async_to_sync):")
    for t in tasks:
        print(f"  [{t.priority}] {t.title}")


#
#  RUN_ASYNC — Run sync function from async context
#
def fetch_external_data() -> dict:
    """Simulate fetching data from an external sync API."""
    time.sleep(0.1)
    return {"status": "ok", "data": [1, 2, 3]}


async def demo_run_async() -> None:
    print("\n" + "=" * 60)
    print("run_async() — Run Sync Functions from Async Code")
    print("=" * 60)

    # Run a blocking sync function without blocking the event loop
    print("Fetching external data (simulated):")
    result = await run_async(fetch_external_data)
    print(f"  Result: {result}")

    # Run multiple sync functions concurrently
    print("\nRunning multiple sync ops concurrently:")
    results = await asyncio.gather(
        run_async(fetch_external_data),
        run_async(fetch_external_data),
        run_async(fetch_external_data),
    )
    print(f"  Got {len(results)} responses")


#
#  PRACTICAL PATTERN: CLI SCRIPT
#
def cli_list_tasks() -> None:
    """CLI command — synchronous entry point."""
    print("\n  CLI: Listing all tasks")
    print("  " + "-" * 40)
    tasks = run_sync(Task.objects.order_by("priority"))
    for t in tasks:
        status = "done" if getattr(t, "is_done", False) else "pending"
        print(f"  [{t.priority}] {t.title} ({status})")
    print(f"\n  Total: {run_sync(Task.objects.count())} tasks")


def cli_add_task(title: str, priority: int = 0) -> None:
    """CLI command — add a task."""

    async def _create():
        return await Task.objects.create(title=title, priority=priority)

    task = run_sync(_create())
    print(f"\n  CLI: Created task '{task.title}' (pk={task.pk})")


async def demo_cli_pattern() -> None:
    print("\n" + "=" * 60)
    print("Practical Pattern: CLI Script")
    print("=" * 60)

    # Simulate CLI commands running in threads (since we're in async context)
    loop = asyncio.get_event_loop()

    # List tasks
    await loop.run_in_executor(None, cli_list_tasks)

    # Add a task
    await loop.run_in_executor(None, lambda: cli_add_task("New CLI Task", 5))

    # List again
    await loop.run_in_executor(None, cli_list_tasks)


#
#  PRACTICAL PATTERN: MIXED CODEBASE
#
class SyncRepository:
    """A synchronous repository that wraps async ORM operations.

    Useful when integrating Ryx into an existing synchronous codebase.
    """

    def get_all(self) -> list:
        return run_sync(Task.objects.all())

    def get_by_priority(self, priority: int) -> list:
        return run_sync(Task.objects.filter(priority=priority))

    def count(self) -> int:
        return run_sync(Task.objects.count())

    def create(self, title: str, priority: int = 0):
        async def _create():
            return await Task.objects.create(title=title, priority=priority)

        return run_sync(_create())


async def demo_repository_pattern() -> None:
    print("\n" + "=" * 60)
    print("Practical Pattern: Sync Repository Layer")
    print("=" * 60)

    repo = SyncRepository()

    # Use the sync repository from async code (via threads)
    loop = asyncio.get_event_loop()

    count = await loop.run_in_executor(None, repo.count)
    print(f"Repository count: {count}")

    high_priority = await loop.run_in_executor(None, repo.get_by_priority, 3)
    print(f"Priority 3 tasks: {[t.title for t in high_priority]}")

    new_task = await loop.run_in_executor(None, repo.create, "Repo Task", 4)
    print(f"Created via repo: {new_task.title}")


#
#  MAIN
#
async def main() -> None:
    print("Ryx ORM — Example 12: Sync/Async Bridge")
    await setup()

    await demo_run_sync()
    await demo_sync_to_async()
    await demo_async_to_sync()
    await demo_run_async()
    await demo_cli_pattern()
    await demo_repository_pattern()

    print("\nDone!")


if __name__ == "__main__":
    asyncio.run(main())
