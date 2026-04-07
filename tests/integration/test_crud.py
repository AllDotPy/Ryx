"""
Integration tests for CRUD operations.
"""

import pytest
from conftest import Author, Post, Tag, PostTag, clean_tables

from ryx.exceptions import ValidationError, MultipleObjectsReturned


class TestCreate:
    """Test create operations."""

    @pytest.mark.asyncio
    async def test_create_simple(self, clean_tables):
        """Test basic object creation."""
        author = await Author.objects.create(name="John Doe", email="john@example.com")

        assert author.pk is not None
        assert author.name == "John Doe"
        assert author.email == "john@example.com"
        assert author.active is True  # default value

    @pytest.mark.asyncio
    async def test_create_with_defaults(self, clean_tables):
        """Test creation with default values."""
        post = await Post.objects.create(title="Test Post", slug="test-post")

        assert post.pk is not None
        assert post.title == "Test Post"
        assert post.views == 0  # default
        assert post.active is True  # default
        assert post.body is None  # null field

    @pytest.mark.asyncio
    async def test_create_multiple(self, clean_tables):
        """Test creating multiple objects."""
        await Author.objects.create(name="Author 1", email="author1@example.com")
        await Author.objects.create(name="Author 2", email="author2@example.com")
        await Author.objects.create(name="Author 3", email="author3@example.com")

        count = await Author.objects.count()
        assert count == 3

    @pytest.mark.asyncio
    async def test_get_or_create_create(self, clean_tables):
        """Test get_or_create when object doesn't exist."""
        author, created = await Author.objects.get_or_create(
            email="new@example.com", defaults={"name": "New Author"}
        )

        assert created is True
        assert author.email == "new@example.com"
        assert author.name == "New Author"

    @pytest.mark.asyncio
    async def test_get_or_create_get(self, clean_tables):
        """Test get_or_create when object exists."""
        existing = await Author.objects.create(
            name="Existing Author", email="existing@example.com"
        )

        author, created = await Author.objects.get_or_create(
            email="existing@example.com", defaults={"name": "Should not be used"}
        )

        assert created is False
        assert author.pk == existing.pk
        assert author.name == "Existing Author"

    @pytest.mark.asyncio
    async def test_update_or_create_create(self, clean_tables):
        """Test update_or_create when object doesn't exist."""
        post, created = await Post.objects.update_or_create(
            slug="new-post", defaults={"title": "New Post", "views": 10}
        )

        assert created is True
        assert post.slug == "new-post"
        assert post.title == "New Post"
        assert post.views == 10

    @pytest.mark.asyncio
    async def test_update_or_create_update(self, clean_tables):
        """Test update_or_create when object exists."""
        existing = await Post.objects.create(
            title="Original Title", slug="test-post", views=5
        )

        post, created = await Post.objects.update_or_create(
            slug="test-post", defaults={"title": "Updated Title", "views": 20}
        )

        assert created is False
        assert post.pk == existing.pk
        assert post.title == "Updated Title"
        assert post.views == 20


class TestRead:
    """Test read operations."""

    @pytest.mark.asyncio
    async def test_get_existing(self, sample_author):
        """Test getting an existing object."""
        author = await Author.objects.get(pk=sample_author.pk)
        assert author.pk == sample_author.pk
        assert author.name == sample_author.name

    @pytest.mark.asyncio
    async def test_get_nonexistent(self, clean_tables):
        """Test getting a nonexistent object."""
        with pytest.raises(Author.DoesNotExist):
            await Author.objects.get(pk=999)

    @pytest.mark.asyncio
    async def test_get_multiple_matches(self, clean_tables):
        """Test get when multiple objects match."""
        await Author.objects.create(name="Same Name", email="email1@example.com")
        await Author.objects.create(name="Same Name", email="email2@example.com")

        with pytest.raises(MultipleObjectsReturned):
            await Author.objects.get(name="Same Name")

    @pytest.mark.asyncio
    async def test_all(self, clean_tables):
        """Test retrieving all objects."""
        await Author.objects.create(name="Author 1", email="author1@example.com")
        await Author.objects.create(name="Author 2", email="author2@example.com")

        authors = await Author.objects.all()
        assert len(authors) == 2

    @pytest.mark.asyncio
    async def test_first(self, clean_tables):
        """Test getting the first object."""
        await Author.objects.create(name="First", email="first@example.com")
        await Author.objects.create(name="Second", email="second@example.com")

        first = await Author.objects.order_by("name").first()
        assert first.name == "First"

    @pytest.mark.asyncio
    async def test_last(self, clean_tables):
        """Test getting the last object."""
        await Author.objects.create(name="First", email="first@example.com")
        await Author.objects.create(name="Second", email="second@example.com")

        last = await Author.objects.order_by("name").last()
        assert last.name == "Second"

    @pytest.mark.asyncio
    async def test_count(self, clean_tables):
        """Test counting objects."""
        await Author.objects.create(name="Author 1", email="author1@example.com")
        await Author.objects.create(name="Author 2", email="author2@example.com")

        count = await Author.objects.count()
        assert count == 2

    @pytest.mark.asyncio
    async def test_exists(self, clean_tables):
        """Test checking if objects exist."""
        assert await Author.objects.exists() is False

        await Author.objects.create(name="Author", email="author@example.com")
        assert await Author.objects.exists() is True


class TestUpdate:
    """Test update operations."""

    @pytest.mark.asyncio
    async def test_save_update(self, sample_author):
        """Test updating an object via save."""
        sample_author.name = "Updated Name"
        await sample_author.save()

        # Fetch again to verify
        updated = await Author.objects.get(pk=sample_author.pk)
        assert updated.name == "Updated Name"

    @pytest.mark.asyncio
    async def test_save_with_validation(self, sample_post):
        """Test that save runs validation by default."""
        sample_post.views = -1  # Invalid

        with pytest.raises(ValidationError):
            await sample_post.save()

    @pytest.mark.asyncio
    async def test_save_skip_validation(self, sample_post):
        """Test saving with validation disabled."""
        sample_post.views = -1  # Invalid but we'll skip validation
        await sample_post.save(validate=False)

        # Should be saved despite invalid data
        updated = await Post.objects.get(pk=sample_post.pk)
        assert updated.views == -1

    @pytest.mark.asyncio
    async def test_queryset_update(self, clean_tables):
        """Test updating multiple objects via QuerySet."""
        await Post.objects.create(title="Post 1", views=10)
        await Post.objects.create(title="Post 2", views=20)

        updated_count = await Post.objects.filter(views__lt=15).update(views=15)
        assert updated_count == 1

        posts = await Post.objects.order_by("title")
        assert posts[0].views == 15
        assert posts[1].views == 20


class TestDelete:
    """Test delete operations."""

    @pytest.mark.asyncio
    async def test_delete_instance(self, sample_author):
        """Test deleting an instance."""
        pk = sample_author.pk
        await sample_author.delete()

        # Should not exist anymore
        with pytest.raises(Author.DoesNotExist):
            await Author.objects.get(pk=pk)

    @pytest.mark.asyncio
    async def test_queryset_delete(self, clean_tables):
        """Test deleting multiple objects via QuerySet."""
        await Post.objects.create(title="Post 1", views=10)
        await Post.objects.create(title="Post 2", views=20)

        deleted_count = await Post.objects.filter(views__lt=15).delete()
        assert deleted_count == 1

        remaining = await Post.objects.count()
        assert remaining == 1
