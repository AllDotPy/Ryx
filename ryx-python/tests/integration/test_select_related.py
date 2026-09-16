"""
Integration and unit tests for select_related functionality (Ryx #64).
"""

import pytest
from conftest import Author, Post, Tag, Q


class TestSelectRelated:
    """Tests for QuerySet.select_related()."""

    def test_select_related_lazy(self):
        """Verify select_related is lazy and does not immediately execute SQL."""
        qs = Post.objects.select_related("author")
        assert qs._select_related == ["author"]
        assert isinstance(qs, type(Post.objects.all()))

    def test_select_related_cloning_and_chaining(self):
        """Verify select_related survives QuerySet cloning and chaining."""
        qs = Post.objects.select_related("author")
        filtered = qs.filter(active=True)
        ordered = filtered.order_by("title")
        limited = ordered.limit(5)

        assert qs._select_related == ["author"]
        assert filtered._select_related == ["author"]
        assert ordered._select_related == ["author"]
        assert limited._select_related == ["author"]

    def test_select_related_rejects_nested(self):
        """Verify nested relationships (author__company) raise ValueError."""
        with pytest.raises(ValueError, match="Nested select_related is not supported"):
            Post.objects.select_related("author__company")

    def test_sql_generation(self):
        """Verify generated SQL includes qualified columns, LEFT OUTER JOIN, and aliases."""
        qs = Post.objects.select_related("author").filter(active=True)
        sql = qs.query

        assert "LEFT OUTER JOIN" in sql
        assert '"test_authors" AS "author"' in sql
        assert '"test_posts"."title"' in sql
        assert '"author"."name" AS "author__name"' in sql

    @pytest.mark.asyncio
    async def test_eager_loading_and_hydration(self, clean_tables):
        """Verify eager loading populates related model instances without extra queries."""
        alice = await Author.objects.create(name="Alice", email="alice@example.com")
        bob = await Author.objects.create(name="Bob", email="bob@example.com")

        p1 = await Post.objects.create(title="Post 1", author=alice)
        p2 = await Post.objects.create(title="Post 2", author=alice)
        p3 = await Post.objects.create(title="Post 3", author=bob)
        p4 = await Post.objects.create(title="Post 4", author=None)

        posts = await Post.objects.select_related("author").order_by("id")
        assert len(posts) == 4

        # Verify related model instances are populated
        assert posts[0].author is not None
        assert posts[0].author.pk == alice.pk
        assert posts[0].author.name == "Alice"

        assert posts[1].author is not None
        assert posts[1].author.pk == alice.pk
        assert posts[1].author.name == "Alice"

        assert posts[2].author is not None
        assert posts[2].author.pk == bob.pk
        assert posts[2].author.name == "Bob"

        # Nullable relationship test
        assert posts[3].author is None

    @pytest.mark.asyncio
    async def test_count_and_exists_ignore_select_related(self, clean_tables):
        """Verify count() and exists() ignore select_related JOINs."""
        alice = await Author.objects.create(name="Alice", email="alice@example.com")
        await Post.objects.create(title="Post 1", author=alice)
        await Post.objects.create(title="Post 2", author=alice)

        qs = Post.objects.select_related("author")
        count = await qs.count()
        assert count == 2

        exists = await qs.exists()
        assert exists is True
