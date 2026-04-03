"""
Integration tests for bulk operations.
"""

import pytest
from conftest import Author, Post, Tag


class TestBulkCreate:
    """Test bulk_create operations."""

    @pytest.mark.asyncio
    async def test_bulk_create_simple(self, clean_tables):
        """Test basic bulk creation."""
        posts = [
            Post(title="Post 1", slug="post-1", views=10),
            Post(title="Post 2", slug="post-2", views=20),
            Post(title="Post 3", slug="post-3", views=30),
        ]

        created_posts = await Post.objects.bulk_create(posts)
        assert len(created_posts) == 3

        # Verify they were created
        all_posts = await Post.objects.order_by("title")
        assert len(all_posts) == 3
        assert [p.title for p in all_posts] == ["Post 1", "Post 2", "Post 3"]
        assert [p.views for p in all_posts] == [10, 20, 30]

    @pytest.mark.asyncio
    async def test_bulk_create_with_defaults(self, clean_tables):
        """Test bulk creation with default values."""
        authors = [
            Author(name="Author 1", email="author1@example.com"),
            Author(name="Author 2", email="author2@example.com"),
        ]

        created_authors = await Author.objects.bulk_create(authors)
        assert len(created_authors) == 2

        # Check defaults were applied
        for author in created_authors:
            assert author.active is True
            assert author.bio is None

    @pytest.mark.asyncio
    async def test_bulk_create_large_batch(self, clean_tables):
        """Test bulk creation with many objects."""
        posts = [Post(title=f"Post {i}", slug=f"post-{i}", views=i) for i in range(100)]

        created_posts = await Post.objects.bulk_create(posts)
        assert len(created_posts) == 100

        count = await Post.objects.count()
        assert count == 100


class TestBulkUpdate:
    """Test bulk_update operations."""

    @pytest.mark.asyncio
    async def test_bulk_update_simple(self, clean_tables):
        """Test basic bulk update."""
        posts = []
        for i in range(5):
            post = await Post.objects.create(
                title=f"Post {i}", slug=f"post-{i}", views=i * 10
            )
            posts.append(post)

        # Modify objects
        for post in posts:
            post.views += 100

        updated_count = await Post.objects.bulk_update(posts, ["views"])
        assert updated_count == 5

        # Verify updates
        all_posts = await Post.objects.order_by("title")
        assert [p.views for p in all_posts] == [100, 110, 120, 130, 140]

    @pytest.mark.asyncio
    async def test_bulk_update_multiple_fields(self, clean_tables):
        """Test bulk update with multiple fields."""
        authors = []
        for i in range(3):
            author = await Author.objects.create(
                name=f"Author {i}", email=f"author{i}@example.com", active=bool(i % 2)
            )
            authors.append(author)

        # Modify multiple fields
        for author in authors:
            author.name = f"Updated {author.name}"
            author.active = True

        updated_authors = await Author.objects.bulk_update(authors, ["name", "active"])

        # Verify updates
        all_authors = await Author.objects.order_by("email")
        assert all(a.name.startswith("Updated") for a in all_authors)
        assert all(a.active for a in all_authors)


class TestBulkDelete:
    """Test bulk_delete operations."""

    @pytest.mark.asyncio
    async def test_bulk_delete_simple(self, clean_tables):
        """Test basic bulk delete."""
        for i in range(5):
            await Post.objects.create(title=f"Post {i}", slug=f"post-{i}", views=i * 10)

        # Delete posts with low views
        deleted_count = await Post.objects.filter(views__lt=30).bulk_delete()
        assert deleted_count == 3

        remaining = await Post.objects.count()
        assert remaining == 2

    @pytest.mark.asyncio
    async def test_bulk_delete_all(self, clean_tables):
        """Test deleting all objects."""
        for i in range(3):
            await Post.objects.create(title=f"Post {i}", slug=f"post-{i}")

        deleted_count = await Post.objects.bulk_delete()
        assert deleted_count == 3

        remaining = await Post.objects.count()
        assert remaining == 0


class TestStream:
    """Test streaming operations."""

    @pytest.mark.asyncio
    async def test_stream_basic(self, clean_tables):
        """Test basic streaming."""
        for i in range(10):
            await Post.objects.create(title=f"Post {i}", slug=f"post-{i}", views=i)

        # Stream all posts
        posts = []
        async for post in Post.objects.stream():
            posts.append(post)

        assert len(posts) == 10

    @pytest.mark.asyncio
    async def test_stream_with_filter(self, clean_tables):
        """Test streaming with filters."""
        for i in range(10):
            await Post.objects.create(title=f"Post {i}", slug=f"post-{i}", views=i)

        # Stream filtered posts
        posts = []
        async for post in Post.objects.filter(views__gte=5).stream():
            posts.append(post)

        assert len(posts) == 5
        assert all(p.views >= 5 for p in posts)

    @pytest.mark.asyncio
    async def test_stream_ordered(self, clean_tables):
        """Test streaming with ordering."""
        for i in [3, 1, 4, 1, 5]:
            await Post.objects.create(
                title=f"Post {i}",
                slug=f"post-{i}-{len(await Post.objects.filter(views=i))}",
                views=i,
            )

        # Stream in order
        posts = []
        async for post in Post.objects.order_by("views").stream():
            posts.append(post)

        views = [p.views for p in posts]
        assert views == sorted(views)


class TestBulkOperationsIntegration:
    """Test bulk operations working together."""

    @pytest.mark.asyncio
    async def test_bulk_workflow(self, clean_tables):
        """Test a complete bulk workflow."""
        # Bulk create
        posts = [
            Post(title=f"Post {i}", slug=f"post-{i}", views=i, active=i % 2 == 0)
            for i in range(10)
        ]
        created_posts = await Post.objects.bulk_create(posts)
        assert len(created_posts) == 10

        # Bulk update inactive posts
        inactive_posts = await Post.objects.filter(active=False)
        for post in inactive_posts:
            post.views += 100
        await Post.objects.bulk_update(inactive_posts, ["views"])

        # Verify updates
        updated_posts = await Post.objects.filter(views__gte=100)
        assert len(updated_posts) == 5

        # Bulk delete old posts
        deleted_count = await Post.objects.filter(views__lt=50).bulk_delete()
        assert deleted_count == 5

        # Final count
        remaining = await Post.objects.count()
        assert remaining == 5
