"""
Integration tests for transaction operations.
"""

import pytest
from conftest import Author, Post, Tag
from ryx import transaction
from ryx.exceptions import ValidationError


class TestTransactionBasics:
    """Test basic transaction operations."""

    @pytest.mark.asyncio
    async def test_transaction_commit(self, clean_tables):
        """Test successful transaction commit."""
        async with transaction():
            await Author.objects.create(name="John", email="john@example.com")
            await Author.objects.create(name="Jane", email="jane@example.com")

        # Verify both were committed
        count = await Author.objects.count()
        assert count == 2

    @pytest.mark.asyncio
    async def test_transaction_rollback_on_exception(self, clean_tables):
        """Test transaction rollback on exception."""
        with pytest.raises(ValueError):
            async with transaction():
                await Author.objects.create(name="John", email="john@example.com")
                raise ValueError("Something went wrong")
                await Author.objects.create(name="Jane", email="jane@example.com")

        # Verify nothing was committed
        count = await Author.objects.count()
        assert count == 0

    @pytest.mark.asyncio
    async def test_nested_transactions(self, clean_tables):
        """Test nested transactions."""
        async with transaction():
            await Author.objects.create(name="Outer", email="outer@example.com")

            async with transaction():
                await Author.objects.create(name="Inner", email="inner@example.com")

            # Inner transaction committed
            inner_count = await Author.objects.count()
            assert inner_count == 2

        # Outer transaction committed
        final_count = await Author.objects.count()
        assert final_count == 2

    @pytest.mark.asyncio
    async def test_nested_transaction_rollback(self, clean_tables):
        """Test rollback of nested transaction."""
        async with transaction():
            await Author.objects.create(name="Outer", email="outer@example.com")

            try:
                async with transaction():
                    await Author.objects.create(name="Inner", email="inner@example.com")
                    raise ValueError("Inner failed")
            except ValueError:
                pass  # Expected

            # Inner transaction rolled back, but outer continues
            count = await Author.objects.count()
            assert count == 1

        # Outer committed
        final_count = await Author.objects.count()
        assert final_count == 1


class TestTransactionIsolation:
    """Test transaction isolation properties."""

    @pytest.mark.asyncio
    async def test_transaction_isolation_read(self, clean_tables):
        """Test that transactions isolate reads."""
        # Create initial data
        await Author.objects.create(name="Initial", email="initial@example.com")

        async with transaction():
            # Inside transaction, create more data
            await Author.objects.create(name="Inside", email="inside@example.com")

            # Should see both inside transaction
            count_inside = await Author.objects.count()
            assert count_inside == 2

        # Outside transaction, should still see both
        count_outside = await Author.objects.count()
        assert count_outside == 2

    @pytest.mark.asyncio
    async def test_transaction_isolation_write(self, clean_tables):
        """Test that transaction writes are isolated."""
        async with transaction():
            await Author.objects.create(name="Txn Author", email="txn@example.com")

            # Inside transaction, should see the new author
            authors = await Author.objects.filter(email="txn@example.com")
            assert len(authors) == 1

        # Outside transaction, should still see the author
        authors = await Author.objects.filter(email="txn@example.com")
        assert len(authors) == 1


class TestTransactionComplexOperations:
    """Test complex operations within transactions."""

    @pytest.mark.asyncio
    async def test_transaction_with_bulk_operations(self, clean_tables):
        """Test bulk operations within transactions."""
        async with transaction():
            # Bulk create
            posts = [
                Post(title=f"Post {i}", slug=f"post-{i}")
                for i in range(5)
            ]
            await Post.objects.bulk_create(posts)

            # Bulk update
            created_posts = await Post.objects.all()
            for post in created_posts:
                post.views = 10
            await Post.objects.bulk_update(created_posts, ["views"])

            # Bulk delete
            await Post.objects.filter(views=10).bulk_delete()

        # Verify transaction committed and all operations worked
        count = await Post.objects.count()
        assert count == 0

    @pytest.mark.asyncio
    async def test_transaction_rollback_bulk_operations(self, clean_tables):
        """Test that bulk operations are rolled back."""
        with pytest.raises(ValueError):
            async with transaction():
                posts = [
                    Post(title=f"Post {i}", slug=f"post-{i}")
                    for i in range(3)
                ]
                await Post.objects.bulk_create(posts)
                raise ValueError("Force rollback")

        # Verify nothing was committed
        count = await Post.objects.count()
        assert count == 0

    @pytest.mark.asyncio
    async def test_transaction_with_relationships(self, clean_tables):
        """Test transactions with related object operations."""
        async with transaction():
            author = await Author.objects.create(
                name="Author",
                email="author@example.com"
            )

            post = await Post.objects.create(
                title="Post",
                slug="post",
                author=author
            )

            # Update both
            author.bio = "Updated bio"
            await author.save()

            post.views = 100
            await post.save()

        # Verify both updates committed
        updated_author = await Author.objects.get(pk=author.pk)
        updated_post = await Post.objects.get(pk=post.pk)

        assert updated_author.bio == "Updated bio"
        assert updated_post.views == 100
        assert updated_post.author.pk == author.pk


class TestTransactionEdgeCases:
    """Test transaction edge cases."""

    @pytest.mark.asyncio
    async def test_transaction_context_manager(self, clean_tables):
        """Test transaction as context manager."""
        async with transaction():
            await Author.objects.create(name="Test", email="test@example.com")

        count = await Author.objects.count()
        assert count == 1

    @pytest.mark.asyncio
    async def test_transaction_multiple_operations(self, clean_tables):
        """Test multiple operations in single transaction."""
        async with transaction():
            # Create
            author = await Author.objects.create(name="Test", email="test@example.com")

            # Read
            fetched = await Author.objects.get(pk=author.pk)
            assert fetched.name == "Test"

            # Update
            fetched.name = "Updated"
            await fetched.save()

            # Delete
            await fetched.delete()

        # Verify final state
        count = await Author.objects.count()
        assert count == 0

    @pytest.mark.asyncio
    async def test_transaction_with_validation_errors(self, clean_tables):
        """Test transactions with validation errors."""
        async with transaction():
            # This should work
            await Post.objects.create(title="Valid Post", slug="valid-post")

            # This should fail validation
            try:
                await Post.objects.create(title="", slug="invalid-post")  # Empty title
            except ValidationError:
                pass  # Expected

            # Transaction should still commit the valid post
            count = await Post.objects.count()
            assert count == 1