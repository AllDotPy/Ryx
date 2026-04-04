"""
Ryx ORM — Example 02: CRUD Operations

This example covers:
  - Creating instances (.create, .save)
  - Reading instances (.get, .first, .last, .all, .filter)
  - Updating instances (.save, .update)
  - Deleting instances (.delete)
  - get_or_create / update_or_create
  - refresh_from_db
  - Per-model DoesNotExist / MultipleObjectsReturned

Run with:
    uv run python examples/02_crud_operations.py
"""

import asyncio
import os
from pathlib import Path

import ryx
from ryx import Model, CharField, IntField, BooleanField, DateTimeField, ForeignKey
from ryx.migrations import MigrationRunner


DB_PATH = Path(__file__).parent.parent / "ryx_examples.sqlite3"
DATABASE_URL = f"sqlite://{DB_PATH}?mode=rwc"
os.environ["RYX_DATABASE_URL"] = DATABASE_URL


#
#  MODELS
#
class Author(Model):
    class Meta:
        table_name = "ex2_authors"

    name = CharField(max_length=100)
    email = CharField(max_length=200, unique=True)
    # Note: Using IntField for boolean-like flags due to SQLite/Any driver
    # compatibility. In production with Postgres, use BooleanField.
    is_active = IntField(default=1)


class Post(Model):
    class Meta:
        table_name = "ex2_posts"

    title = CharField(max_length=200)
    body = CharField(max_length=500, null=True, blank=True)
    views = IntField(default=0)
    author = ForeignKey(Author, null=True, on_delete="SET_NULL")
    created_at = DateTimeField(auto_now_add=True)


async def setup() -> None:
    await ryx.setup(DATABASE_URL)
    runner = MigrationRunner([Author, Post])
    await runner.migrate()

    # Clean tables for fresh demo
    await Post.objects.bulk_delete()
    await Author.objects.bulk_delete()


#
#  CREATE
#
async def demo_create() -> None:
    print("\n" + "=" * 60)
    print("CREATE Operations")
    print("=" * 60)

    # 1. Manager.create() — one-liner to create and save
    alice = await Author.objects.create(
        name="Alice Martin",
        email="alice@example.com",
    )
    print(f"Created author: {alice} (pk={alice.pk})")

    # 2. Constructor + save() — more control, useful for hooks
    bob = Author(name="Bob Dupont", email="bob@example.com")
    await bob.save()
    print(f"Created author: {bob} (pk={bob.pk})")

    # 3. Create with defaults applied automatically
    charlie = await Author.objects.create(name="Charlie", email="charlie@example.com")
    print(f"Created author (is_active={bool(charlie.is_active)}): {charlie}")

    # 4. Create related objects
    post1 = await Post.objects.create(
        title="Introduction to Ryx",
        body="Ryx is a fast async ORM...",
        views=42,
        author=alice,
    )
    print(f"Created post: {post1} by {post1.author_id}")


#
#  READ
#
async def demo_read() -> None:
    print("\n" + "=" * 60)
    print("READ Operations")
    print("=" * 60)

    # 1. .get() — fetch exactly one by any field
    alice = await Author.objects.get(email="alice@example.com")
    print(f"get() → {alice}")

    # 2. .get() with pk shorthand
    alice_by_pk = await Author.objects.get(pk=alice.pk)
    print(f"get(pk=…) → {alice_by_pk}")

    # 3. .first() / .last() — ordered results
    first_author = await Author.objects.order_by("name").first()
    last_author = await Author.objects.order_by("name").last()
    print(f"first() → {first_author.name}")
    print(f"last()  → {last_author.name}")

    # 4. .all() — fetch all as a list of model instances
    all_authors = await Author.objects.all()
    print(f"all() → {len(all_authors)} authors: {[a.name for a in all_authors]}")

    # 5. .filter() — returns a list
    active_authors = await Author.objects.filter(is_active=True)
    print(f"filter(is_active=True) → {len(active_authors)} authors")

    # 6. .count() — efficient COUNT query
    total = await Author.objects.count()
    print(f"count() → {total}")

    # 7. .exists() — efficient EXISTS query
    has_authors = await Author.objects.filter(name__startswith="Alice").exists()
    print(f"exists(Alice) → {has_authors}")

    # 8. DoesNotExist / MultipleObjectsReturned — per-model exceptions
    try:
        await Author.objects.get(email="nobody@example.com")
    except Author.DoesNotExist:
        print("get() raised Author.DoesNotExist (expected)")

    # 9. refresh_from_db — reload instance state from database
    await Author.objects.filter(pk=alice.pk).update(is_active=False)
    print(f"Before refresh: alice.is_active = {alice.is_active}")
    await alice.refresh_from_db()
    print(f"After refresh:  alice.is_active = {alice.is_active}")


#
#  UPDATE
#
async def demo_update() -> None:
    print("\n" + "=" * 60)
    print("UPDATE Operations")
    print("=" * 60)

    alice = await Author.objects.get(email="alice@example.com")

    # 1. Instance .save() — modify attributes and save
    alice.name = "Alice M."
    await alice.save()
    print(f"save() → {alice}")

    # 2. Instance .save(update_fields=…) — only UPDATE specified columns
    alice.name = "Alice Martin"
    alice.is_active = False
    await alice.save(update_fields=["name"])
    # is_active is NOT updated
    await alice.refresh_from_db()
    print(f"save(update_fields=['name']) → is_active still = {alice.is_active}")

    # 3. QuerySet .update() — bulk UPDATE in a single SQL statement
    count = await Author.objects.filter(is_active=False).update(is_active=True)
    print(f"update() → {count} authors reactivated")

    # 4. get_or_create — fetch or insert atomically
    obj, created = await Author.objects.get_or_create(
        email="newbie@example.com",
        defaults={"name": "New User"},
    )
    print(f"get_or_create → created={created}, {obj}")

    # Try again — should return existing
    obj2, created2 = await Author.objects.get_or_create(
        email="newbie@example.com",
        defaults={"name": "Should Not Change"},
    )
    print(f"get_or_create (again) → created={created2}, {obj2}")

    # 5. update_or_create — update existing or create new
    obj3, created3 = await Author.objects.update_or_create(
        email="newbie@example.com",
        defaults={"name": "Updated Name"},
    )
    print(f"update_or_create → created={created3}, name={obj3.name}")


#
#  DELETE
#
async def demo_delete() -> None:
    print("\n" + "=" * 60)
    print("DELETE Operations")
    print("=" * 60)

    # 1. Instance .delete()
    temp = await Author.objects.create(name="Temp", email="temp@example.com")
    print(f"Before delete: {await Author.objects.count()} authors")
    await temp.delete()
    print(f"After delete:  {await Author.objects.count()} authors")

    # 2. QuerySet .delete() — bulk delete
    # Create some posts to delete
    for i in range(3):
        await Post.objects.create(title=f"Old Post {i}", views=0)

    count_before = await Post.objects.count()
    deleted = await Post.objects.filter(title__startswith="Old Post").delete()
    count_after = await Post.objects.count()
    print(f"bulk_delete() → deleted={deleted}, {count_before} → {count_after}")


#
#  MAIN
#
async def main() -> None:
    print("Ryx ORM — Example 02: CRUD Operations")
    await setup()

    await demo_create()
    await demo_read()
    await demo_update()
    await demo_delete()

    print("\nDone!")


if __name__ == "__main__":
    asyncio.run(main())
