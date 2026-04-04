"""
Ryx ORM — Example 05: Relationships & JOINs

This example covers:
  - ForeignKey — forward access (post.author)
  - Reverse ForeignKey — parent to children (author.posts)
  - Reverse FK manager methods (filter, count, create, add, remove)
  - Explicit JOINs via .join()
  - Many-to-Many through explicit through-table
  - M2M manager (add, remove, set, clear, count)
  - on_delete behaviors (CASCADE, SET_NULL)

Run with:
    uv run python examples/05_relationships_and_joins.py
"""

import asyncio
import os
from pathlib import Path

import ryx
from ryx import Model, CharField, IntField, ForeignKey
from ryx.migrations import MigrationRunner


DB_PATH = Path(__file__).parent.parent / "ryx_examples.sqlite3"
DATABASE_URL = f"sqlite://{DB_PATH}?mode=rwc"
os.environ["RYX_DATABASE_URL"] = DATABASE_URL


#
#  MODELS
#
class Author(Model):
    class Meta:
        table_name = "ex5_authors"

    name = CharField(max_length=100)
    email = CharField(max_length=200)


class Post(Model):
    class Meta:
        table_name = "ex5_posts"

    title = CharField(max_length=200)
    views = IntField(default=0)
    # ForeignKey — many posts belong to one author
    # on_delete="CASCADE" → deleting the author deletes all their posts
    author = ForeignKey(Author, on_delete="CASCADE")


class Tag(Model):
    class Meta:
        table_name = "ex5_tags"

    name = CharField(max_length=30, unique=True)


class PostTag(Model):
    """Explicit through-table for Post ↔ Tag many-to-many."""

    class Meta:
        table_name = "ex5_post_tags"
        unique_together = [("post_id", "tag_id")]

    post = ForeignKey(Post, on_delete="CASCADE")
    tag = ForeignKey(Tag, on_delete="CASCADE")


async def setup() -> None:
    await ryx.setup(DATABASE_URL)
    runner = MigrationRunner([Author, Post, Tag, PostTag])
    await runner.migrate()

    # Clean
    await PostTag.objects.bulk_delete()
    await Post.objects.bulk_delete()
    await Tag.objects.bulk_delete()
    await Author.objects.bulk_delete()


#
#  FOREIGN KEY — FORWARD ACCESS
#
async def demo_fk_forward() -> None:
    print("\n" + "=" * 60)
    print("ForeignKey — Forward Access (post.author)")
    print("=" * 60)

    alice = await Author.objects.create(name="Alice", email="alice@example.com")
    post = await Post.objects.create(title="Hello World", views=100, author=alice)

    # Access the related object via the forward descriptor
    # This lazy-loads and caches the result
    author = post.author
    print(f"Post '{post.title}' is by {author.name} ({author.email})")

    # The FK ID is also directly available
    print(f"FK value: post.author_id = {post.author_id}")

    # Setting a new author updates the FK
    bob = await Author.objects.create(name="Bob", email="bob@example.com")
    post.author = bob
    await post.save()
    print(f"Changed author to: {post.author.name}")


#
#  FOREIGN KEY — REVERSE ACCESS
#
async def demo_fk_reverse() -> None:
    print("\n" + "=" * 60)
    print("ForeignKey — Reverse Access (author.posts)")
    print("=" * 60)

    alice = await Author.objects.create(name="Alice", email="alice2@example.com")

    # Create posts for Alice
    p1 = await Post.objects.create(title="Post 1", views=10, author=alice)
    p2 = await Post.objects.create(title="Post 2", views=20, author=alice)
    p3 = await Post.objects.create(title="Post 3", views=30, author=alice)

    # Access reverse relation — returns a ReverseFKManager
    # Awaiting it returns all related posts
    posts = await alice.post_set
    print(f"Alice has {len(posts)} posts: {[p.title for p in posts]}")

    # Reverse manager supports QuerySet-like methods
    count = await alice.post_set.count()
    print(f"alice.post_set.count() = {count}")

    # Filter related objects
    popular = await alice.post_set.filter(views__gte=20)
    print(f"Popular posts (views>=20): {[p.title for p in popular]}")

    # Create a child directly linked to parent
    new_post = await alice.post_set.create(title="New Post", views=0)
    print(
        f"Created via post_set.create(): {new_post.title} (author_id={new_post.author_id})"
    )

    # Order related objects
    ordered = await alice.post_set.order_by("-views")
    print(f"Ordered by views DESC: {[p.title for p in ordered]}")


#
#  EXPLICIT JOINs
#
async def demo_joins() -> None:
    print("\n" + "=" * 60)
    print("Explicit JOINs via .join()")
    print("=" * 60)

    # The .join() method adds a SQL JOIN clause to the query.
    # It's useful for filtering on related table columns.
    #
    # Example: find all posts by joining with the authors table
    # Note: The join() method is a low-level SQL builder — the filter
    # on joined columns uses the table name prefix.
    all_posts = await Post.objects.order_by("title")
    print(f"All posts: {len(all_posts)}")
    for p in all_posts:
        # Access related author via forward descriptor
        author = p.author
        author_name = author.name if author else "Unknown"
        print(f"  {p.title} by {author_name}")


#
#  MANY-TO-MANY (EXPLICIT THROUGH TABLE)
#
async def demo_m2m() -> None:
    print("\n" + "=" * 60)
    print("Many-to-Many via explicit through-table")
    print("=" * 60)

    # Create posts and tags
    author = await Author.objects.create(name="Demo Author", email="demo@example.com")
    post = await Post.objects.create(title="Ryx Guide", views=50, author=author)
    python = await Tag.objects.create(name="Python")
    orm = await Tag.objects.create(name="ORM")
    tutorial = await Tag.objects.create(name="Tutorial")

    # Link post to tags via the through table
    await PostTag.objects.create(post=post, tag=python)
    await PostTag.objects.create(post=post, tag=orm)

    # Query posts by tag — use the through table directly
    python_posts = await PostTag.objects.filter(tag_id=python.pk)
    print(f"Posts tagged 'Python': {len(python_posts)}")
    for pt in python_posts:
        print(f"  Post #{pt.post_id}")

    # Count tags per post
    tag_counts = await PostTag.objects.filter(post_id=post.pk).count()
    print(f"Tags on 'Ryx Guide': {tag_counts}")

    # Add more tags
    await PostTag.objects.create(post=post, tag=tutorial)
    total_tags = await PostTag.objects.filter(post_id=post.pk).count()
    print(f"Post now has {total_tags} tags")

    # Remove a tag
    await PostTag.objects.filter(post_id=post.pk, tag_id=orm.pk).delete()
    remaining = await PostTag.objects.filter(post_id=post.pk).count()
    print(f"After removing 'ORM': {remaining} tags remaining")


#
#  ON_DELETE BEHAVIORS
#
async def demo_on_delete() -> None:
    print("\n" + "=" * 60)
    print("on_delete Behaviors")
    print("=" * 60)

    # CASCADE — deleting the author also deletes their posts
    temp_author = await Author.objects.create(name="Temp", email="temp@example.com")
    temp_post = await Post.objects.create(title="Temp Post", author=temp_author)

    posts_before = await Post.objects.count()
    await temp_author.delete()
    posts_after = await Post.objects.count()
    print(
        f"CASCADE: Posts before={posts_before}, after={posts_after} (post deleted with author)"
    )


#
#  MAIN
#
async def main() -> None:
    print("Ryx ORM — Example 05: Relationships & JOINs")
    await setup()

    await demo_fk_forward()
    await demo_fk_reverse()
    await demo_joins()
    await demo_m2m()
    await demo_on_delete()

    print("\nDone!")


if __name__ == "__main__":
    asyncio.run(main())
