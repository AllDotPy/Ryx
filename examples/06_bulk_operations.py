"""
Ryx ORM — Example 06: Bulk Operations

This example covers:
  - bulk_create — multi-row INSERT in batches
  - bulk_update — update many instances efficiently
  - bulk_delete — delete many instances at once
  - stream() — async generator for memory-efficient iteration
  - QuerySet.bulk_delete() — delete all matching rows
  - Performance comparison: bulk vs individual operations

Run with:
    uv run python examples/06_bulk_operations.py
"""

import asyncio
import os
import time
from pathlib import Path

import ryx
from ryx import Model, CharField, IntField, BooleanField
from ryx.migrations import MigrationRunner


DB_PATH = Path(__file__).parent.parent / "ryx_examples.sqlite3"
DATABASE_URL = f"sqlite://{DB_PATH}?mode=rwc"
os.environ["RYX_DATABASE_URL"] = DATABASE_URL


#
#  MODELS
#
class Item(Model):
    class Meta:
        table_name = "ex6_items"

    name = CharField(max_length=100)
    price = IntField(default=0)
    category = CharField(max_length=50, default="general")
    is_active = BooleanField(default=True)


async def setup() -> None:
    await ryx.setup(DATABASE_URL)
    runner = MigrationRunner([Item])
    await runner.migrate()

    # Clean
    await Item.objects.bulk_delete()


#
#  BULK CREATE
#
async def demo_bulk_create() -> None:
    print("\n" + "=" * 60)
    print("bulk_create — Multi-row INSERT")
    print("=" * 60)

    # Build 1000 items in memory
    items = [
        Item(name=f"Item {i}", price=i * 10, category="bulk" if i % 2 == 0 else "sale")
        for i in range(1000)
    ]

    # Single multi-row INSERT (batched internally)
    created = await Item.objects.bulk_create(items, batch_size=500)
    print(f"bulk_create: inserted {len(created)} items")

    # Verify
    count = await Item.objects.count()
    print(f"Total items in DB: {count}")

    # With defaults applied
    for item in created[:2]:
        print(f"  {item.name}: price={item.price}, is_active={item.is_active}")


#
#  BULK UPDATE
#
async def demo_bulk_update() -> None:
    print("\n" + "=" * 60)
    print("bulk_update — Efficient multi-row UPDATE")
    print("=" * 60)

    # Fetch items to update
    sale_items = await Item.objects.filter(category="sale")
    print(f"Items in 'sale' category: {len(sale_items)}")

    # Modify in Python
    for item in sale_items:
        item.price = int(item.price * 0.8)  # 20% discount
        item.is_active = True

    # Bulk update only the changed fields
    updated_count = await Item.objects.bulk_update(
        sale_items, fields=["price", "is_active"]
    )
    print(f"bulk_update: updated {updated_count} items")

    # Verify — fetch fresh data
    updated = await Item.objects.filter(category="sale").first()
    print(f"Sample updated item: {updated.name}, price={updated.price}")


#
#  BULK DELETE
#
async def demo_bulk_delete() -> None:
    print("\n" + "=" * 60)
    print("bulk_delete — Delete many instances")
    print("=" * 60)

    count_before = await Item.objects.count()

    # Delete via QuerySet — single DELETE WHERE
    deleted = await Item.objects.filter(category="sale").delete()
    print(f"QuerySet.delete(): deleted {deleted} items")

    count_after = await Item.objects.count()
    print(f"Remaining: {count_after} (was {count_before})")

    # bulk_delete on Manager with explicit instances
    inactive_items = await Item.objects.filter(is_active=False)
    if inactive_items:
        deleted = await Item.objects.bulk_delete(inactive_items)
        print(f"bulk_delete(instances): deleted {deleted} inactive items")
    else:
        print("No inactive items to delete")

    # bulk_delete without arguments — delete ALL
    # (commented out to preserve data for other demos)
    # await Item.objects.bulk_delete()


#
#  STREAM — Memory-efficient iteration
#
async def demo_stream() -> None:
    print("\n" + "=" * 60)
    print("stream() — Async generator for large result sets")
    print("=" * 60)

    # Stream all items in chunks of 100
    processed = 0
    async for item in Item.objects.stream(chunk_size=100):
        processed += 1
        # Process each item without loading everything into memory
    print(f"Streamed {processed} items (chunk_size=100)")

    # Stream with filter
    bulk_count = 0
    async for item in Item.objects.filter(category="bulk").stream(chunk_size=50):
        bulk_count += 1
    print(f"Streamed {bulk_count} 'bulk' items (chunk_size=50)")

    # Stream with ordering
    first_five = []
    async for item in Item.objects.order_by("name").stream(chunk_size=10):
        first_five.append(item.name)
        if len(first_five) >= 5:
            break
    print(f"First 5 alphabetically: {first_five}")


#
#  PERFORMANCE COMPARISON
#
async def demo_performance() -> None:
    print("\n" + "=" * 60)
    print("Performance: Bulk vs Individual Operations")
    print("=" * 60)

    # Clean slate
    await Item.objects.bulk_delete()

    N = 200

    # Individual inserts
    start = time.monotonic()
    for i in range(N):
        await Item.objects.create(name=f"Indiv {i}", price=i)
    individual_time = time.monotonic() - start
    print(f"Individual inserts ({N}): {individual_time:.3f}s")

    # Clean
    await Item.objects.bulk_delete()

    # Bulk insert
    items = [Item(name=f"Bulk {i}", price=i) for i in range(N)]
    start = time.monotonic()
    await Item.objects.bulk_create(items)
    bulk_time = time.monotonic() - start
    print(f"Bulk insert ({N}):      {bulk_time:.3f}s")

    print(f"Speedup: {individual_time / bulk_time:.1f}x faster")


#
#  MAIN
#
async def main() -> None:
    print("Ryx ORM — Example 06: Bulk Operations")
    await setup()

    await demo_bulk_create()
    await demo_bulk_update()
    await demo_bulk_delete()
    await demo_stream()
    await demo_performance()

    print("\nDone!")


if __name__ == "__main__":
    asyncio.run(main())
