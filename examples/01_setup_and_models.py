"""
Ryx ORM — Example 01: Setup & Model Definitions

This example covers:
  - Database connection setup
  - Defining models with various field types
  - Meta options (table_name, ordering, indexes, constraints)
  - Abstract models and inheritance
  - Auto-generated primary keys
  - Custom table and column names

Run with:
    uv run python examples/01_setup_and_models.py
"""

import asyncio
import os
from pathlib import Path

import ryx
from ryx import (
    Model,
    CharField,
    TextField,
    IntField,
    BooleanField,
    FloatField,
    DateTimeField,
    EmailField,
    URLField,
    UUIDField,
    JSONField,
    ForeignKey,
    AutoField,
    Index,
    Constraint,
)


#
#  DATABASE SETUP
#
# Use SQLite for examples — swap the URL for Postgres or MySQL in production.
DB_PATH = Path(__file__).parent.parent / "ryx_examples.sqlite3"
DATABASE_URL = f"sqlite://{DB_PATH}?mode=rwc"
os.environ["RYX_DATABASE_URL"] = DATABASE_URL


async def setup() -> None:
    """Initialize the connection pool and create tables."""

    # setup() must be called once at application startup.
    # It creates the async connection pool with sensible defaults.
    await ryx.setup(
        DATABASE_URL,
        max_connections=10,  # Max open connections in the pool
        min_connections=1,  # Minimum idle connections kept alive
        connect_timeout=30,  # Seconds to wait for a connection
        idle_timeout=600,  # Seconds before an idle connection is closed
        max_lifetime=1800,  # Max lifetime of any single connection
    )

    # Run migrations to create tables from model definitions
    from ryx.migrations import MigrationRunner

    # Pass all models that need tables
    runner = MigrationRunner([Author, Category, Post, Tag, PostTag, Profile, AuditLog])
    await runner.migrate()


# 
#  BASIC MODEL
# 
class Author(Model):
    """A blog author.

    No primary key is declared — Ryx auto-adds ``id = AutoField()``.
    The table name is auto-derived: ``Author`` → ``"authors"``.
    """

    name = CharField(max_length=100)
    email = EmailField(unique=True)
    bio = TextField(null=True, blank=True)
    is_active = BooleanField(default=True)
    created_at = DateTimeField(auto_now_add=True)

    class Meta:
        ordering = ["-created_at"]
        indexes = [
            Index(fields=["email"], name="author_email_idx"),
        ]


# 
#  MODEL WITH CUSTOM TABLE NAME, INDEXES & CONSTRAINTS
# 
class Category(Model):
    """Blog post category with custom table name and constraints."""

    class Meta:
        table_name = "blog_categories"
        indexes = [
            Index(fields=["slug"], name="category_slug_idx", unique=True),
        ]
        constraints = [
            Constraint(check="LENGTH(name) > 0", name="category_name_nonempty"),
        ]

    name = CharField(max_length=50)
    slug = CharField(max_length=50, unique=True)
    description = TextField(null=True, blank=True)


# 
#  MODEL WITH VARIOUS FIELD TYPES
# 
class Post(Model):
    """A blog post demonstrating many field types and Meta options."""

    class Meta:
        ordering = ["-published_at"]
        unique_together = [("author_id", "slug")]
        indexes = [
            Index(fields=["title"], name="post_title_idx"),
            Index(fields=["published_at"], name="post_published_idx"),
        ]

    # CharField with min/max length
    title = CharField(max_length=200, min_length=3)

    # SlugField — auto-validates slug format
    slug = CharField(max_length=200, unique=True, null=True, blank=True)

    # TextField for long content
    body = TextField()

    # Numeric fields with range constraints
    views = IntField(default=0, min_value=0)
    rating = FloatField(default=0.0, min_value=0.0, max_value=5.0)

    # Boolean with default
    is_published = BooleanField(default=False)

    # URLField — validates URL format
    cover_url = URLField(null=True, blank=True)

    # UUIDField — auto-generates uuid4
    uuid = UUIDField(auto_create=True, unique=True)

    # JSONField — stores arbitrary JSON
    metadata = JSONField(null=True, blank=True, default=dict)

    # DateTimeField with auto timestamps
    published_at = DateTimeField(null=True, blank=True)
    created_at = DateTimeField(auto_now_add=True)
    updated_at = DateTimeField(auto_now=True)

    # ForeignKey — many-to-one relationship
    author = ForeignKey(Author, on_delete="CASCADE")
    category = ForeignKey(Category, null=True, blank=True, on_delete="SET_NULL")


# 
#  MANY-TO-MANY THROUGH TABLE
# 
class Tag(Model):
    """A simple tag."""

    class Meta:
        table_name = "blog_tags"

    name = CharField(max_length=30, unique=True)


class PostTag(Model):
    """Explicit through-table for Post ↔ Tag many-to-many."""

    class Meta:
        table_name = "post_tags"
        unique_together = [("post_id", "tag_id")]

    post = ForeignKey(Post, on_delete="CASCADE")
    tag = ForeignKey(Tag, on_delete="CASCADE")


# 
#  ABSTRACT MODEL & INHERITANCE
# 
class TimestampedModel(Model):
    """Abstract base model — no table is created for this class.

    Child models inherit ``created_at`` and ``updated_at`` fields.
    """

    class Meta:
        abstract = True

    created_at = DateTimeField(auto_now_add=True)
    updated_at = DateTimeField(auto_now=True)


class Profile(TimestampedModel):
    """Inherits from TimestampedModel — gets created_at/updated_at for free."""

    class Meta:
        table_name = "user_profiles"

    user_id = IntField(unique=True)
    display_name = CharField(max_length=100)
    avatar_url = URLField(null=True, blank=True)


# 
#  CUSTOM PRIMARY KEY
# 
class AuditLog(Model):
    """Model with an explicit custom primary key instead of auto id."""

    class Meta:
        table_name = "audit_logs"
        ordering = ["-timestamp"]

    # Override the default AutoField PK
    id = AutoField(primary_key=True)

    action = CharField(max_length=50)
    target = CharField(max_length=200)
    timestamp = DateTimeField(auto_now_add=True)
    details = JSONField(null=True, blank=True)


# 
#  MAIN — Run the example
# 
async def main() -> None:
    print("=" * 60)
    print("Ryx ORM — Example 01: Setup & Model Definitions")
    print("=" * 60)

    # 1. Setup connection pool
    await setup()
    print(f"\nConnected to: {DATABASE_URL}")
    print(f"Pool stats:   {ryx.pool_stats()}")

    # 2. Inspect model metadata
    print("\n--- Model Metadata ---")

    for model in [Author, Category, Post, Tag, PostTag, Profile, AuditLog]:
        meta = model._meta
        print(f"\n{model.__name__}:")
        print(f"  Table:      {meta.table_name}")
        print(f"  Fields:     {list(meta.fields.keys())}")
        print(f"  PK:         {meta.pk_field.attname} ({meta.pk_field.db_type()})")
        if meta.ordering:
            print(f"  Ordering:   {meta.ordering}")
        if meta.indexes:
            print(f"  Indexes:    {[i.name for i in meta.indexes]}")
        if meta.unique_together:
            print(f"  Unique:     {meta.unique_together}")
        if meta.constraints:
            print(f"  Constraints:{[c.name for c in meta.constraints]}")
        if meta.abstract:
            print("  Abstract:   Yes")

    # 3. Verify tables exist
    count = await Author.objects.count()
    print(f"\nAuthors table exists (count={count})")

    count = await Post.objects.count()
    print(f"Posts table exists (count={count})")

    print("\nDone! All models and tables are ready.")


if __name__ == "__main__":
    asyncio.run(main())
