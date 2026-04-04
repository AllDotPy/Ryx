"""
Ryx ORM — Example 10: Caching

This example covers:
  - MemoryCache — built-in in-memory LRU cache
  - configure_cache — set up the global cache backend
  - QuerySet.cache() — cache query results with TTL
  - Named cache keys for manual invalidation
  - Auto-invalidation on save/delete/update
  - invalidate(), invalidate_model(), invalidate_all()
  - Cache statistics and inspection

Run with:
    uv run python examples/10_caching.py
"""

import asyncio
import os
from pathlib import Path

import ryx
from ryx import Model, CharField, IntField
from ryx import (
    MemoryCache,
    configure_cache,
    invalidate,
    invalidate_model,
    invalidate_all,
    get_cache,
)
from ryx.migrations import MigrationRunner


DB_PATH = Path(__file__).parent.parent / "ryx_examples.sqlite3"
DATABASE_URL = f"sqlite://{DB_PATH}?mode=rwc"
os.environ["RYX_DATABASE_URL"] = DATABASE_URL


#
#  MODELS
#
class Setting(Model):
    """Application settings — good candidate for caching."""

    class Meta:
        table_name = "ex10_settings"

    key = CharField(max_length=50, unique=True)
    value = CharField(max_length=200)


class Product(Model):
    """Product catalog — benefit from query caching."""

    class Meta:
        table_name = "ex10_products"

    name = CharField(max_length=100)
    price = IntField(default=0)
    category = CharField(max_length=50)


async def setup() -> None:
    await ryx.setup(DATABASE_URL)
    runner = MigrationRunner([Setting, Product])
    await runner.migrate()

    # Clean
    await Product.objects.bulk_delete()
    await Setting.objects.bulk_delete()

    # Configure cache: 100 entries max, 60s default TTL, auto-invalidate on model changes
    configure_cache(MemoryCache(max_size=100, ttl=60), auto_invalidate=True)

    # Seed data
    settings = [
        Setting(key="site_name", value="My Shop"),
        Setting(key="max_items", value="100"),
        Setting(key="theme", value="dark"),
    ]
    await Setting.objects.bulk_create(settings)

    products = [
        Product(name="Laptop", price=1200, category="Electronics"),
        Product(name="Mouse", price=25, category="Electronics"),
        Product(name="Desk", price=500, category="Furniture"),
        Product(name="Chair", price=300, category="Furniture"),
        Product(name="Notebook", price=5, category="Stationery"),
    ]
    await Product.objects.bulk_create(products)


#
#  BASIC CACHING
#
async def demo_basic_cache() -> None:
    print("\n" + "=" * 60)
    print("Basic Query Caching")
    print("=" * 60)

    cache = get_cache()
    print(f"Cache backend: {type(cache).__name__}")
    print(f"Cache size before: {cache.size()}")

    # First call — hits the database
    print("\nFirst call (DB hit):")
    settings = await Setting.objects.all().cache()
    # .cache() returns dicts from the cache backend
    if settings and isinstance(settings[0], dict):
        print(f"  Settings: {[(s['key'], s['value']) for s in settings]}")
    else:
        print(f"  Settings: {[(s.key, s.value) for s in settings]}")
    print(f"  Cache size after: {cache.size()}")

    # Second call — hits the cache
    print("\nSecond call (cache hit):")
    settings2 = await Setting.objects.all().cache()
    if settings2 and isinstance(settings2[0], dict):
        print(f"  Settings: {[(s['key'], s['value']) for s in settings2]}")
    else:
        print(f"  Settings: {[(s.key, s.value) for s in settings2]}")
    print(f"  Cache size: {cache.size()} (unchanged)")

    # Verify it's the same objects (cached)
    print(f"  Same list? {settings is settings2}")


#
#  NAMED CACHE KEYS
#
async def demo_named_cache() -> None:
    print("\n" + "=" * 60)
    print("Named Cache Keys — Manual Invalidation")
    print("=" * 60)

    cache = get_cache()
    initial_size = cache.size()

    # Cache with a named key
    print("Caching with key='all_products':")
    products = await Product.objects.all().cache(key="all_products", ttl=300)
    print(f"  Products: {[p.name for p in products]}")
    print(f"  Cache size: {cache.size()}")

    # Invalidate by key
    print("\nInvalidating key='all_products':")
    await invalidate("all_products")
    print(f"  Cache size after invalidation: {cache.size()}")


#
#  AUTO-INVALIDATION
#
async def demo_auto_invalidation() -> None:
    print("\n" + "=" * 60)
    print("Auto-Invalidation on Model Changes")
    print("=" * 60)

    cache = get_cache()

    # Cache a query
    print("Caching Product.objects.all():")
    products = await Product.objects.all().cache()
    print(f"  Cached: {len(products)} products")
    print(f"  Cache size: {cache.size()}")

    # Modify a product — should auto-invalidate Product cache
    print("\nUpdating a product (triggers auto-invalidation):")
    laptop = await Product.objects.get(name="Laptop")
    laptop.price = 1100
    await laptop.save()
    print(f"  Laptop price updated to ${laptop.price}")
    print(f"  Cache size after save: {cache.size()} (Product entries invalidated)")


#
#  MODEL-LEVEL INVALIDATION
#
async def demo_model_invalidation() -> None:
    print("\n" + "=" * 60)
    print("Model-Level Invalidation")
    print("=" * 60)

    cache = get_cache()

    # Cache multiple queries
    print("Caching multiple Product queries:")
    await Product.objects.all().cache()
    await Product.objects.filter(category="Electronics").cache()
    await Product.objects.filter(category="Furniture").cache()
    print(f"  Cache size: {cache.size()}")

    # Invalidate all cached queries for Product model
    print("\nInvalidating all Product cache entries:")
    await invalidate_model(Product)
    print(f"  Cache size after: {cache.size()}")


#
#  CACHE WITH TTL
#
async def demo_ttl() -> None:
    print("\n" + "=" * 60)
    print("Cache TTL (Time-To-Live)")
    print("=" * 60)

    cache = get_cache()

    # Cache with a short TTL
    print("Caching with ttl=2 seconds:")
    settings = await Setting.objects.all().cache(ttl=2)
    print(f"  Cached: {len(settings)} settings")

    # Check cache keys
    keys = await cache.keys()
    setting_keys = [k for k in keys if "Setting" in k]
    print(f"  Setting cache keys: {len(setting_keys)}")

    # Wait for TTL to expire
    print("\nWaiting 3 seconds for TTL to expire...")
    await asyncio.sleep(3)

    # The entry should be expired now
    keys_after = await cache.keys()
    setting_keys_after = [k for k in keys_after if "Setting" in k]
    print(f"  Setting cache keys after TTL: {len(setting_keys_after)}")


#
#  CLEAR ALL CACHE
#
async def demo_clear_all() -> None:
    print("\n" + "=" * 60)
    print("Clear All Cache")
    print("=" * 60)

    cache = get_cache()

    # Fill cache
    await Setting.objects.all().cache()
    await Product.objects.all().cache()
    await Product.objects.filter(category="Electronics").cache()
    print(f"Cache size before clear: {cache.size()}")

    # Clear everything
    print("Clearing all cache entries...")
    await invalidate_all()
    print(f"Cache size after clear: {cache.size()}")


#
#  MAIN
#
async def main() -> None:
    print("Ryx ORM — Example 10: Caching")
    await setup()

    await demo_basic_cache()
    await demo_named_cache()
    await demo_auto_invalidation()
    await demo_model_invalidation()
    await demo_ttl()
    await demo_clear_all()

    print("\nDone!")


if __name__ == "__main__":
    asyncio.run(main())
