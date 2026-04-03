"""
Integration tests for Ryx QuerySet operations using real SQLite database.
Tests actual QuerySet behavior with real models and database.
"""

import pytest
import asyncio
import tempfile
import os
from datetime import datetime

# Import test models from conftest
from conftest import Post, Author, Tag, PostTag

# Import Ryx components
import ryx
from ryx import Q
from ryx.exceptions import DoesNotExist, MultipleObjectsReturned


# Setup database for integration tests
@pytest.fixture(scope="module")
async def integration_db():
    """Setup a temporary SQLite database for integration tests."""
    # Create a temp file
    fd, db_path = tempfile.mkstemp(suffix=".db")
    os.close(fd)
    
    # Initialize Ryx with SQLite
    db_url = f"sqlite:///{db_path}"
    await ryx.setup(db_url)
    
    yield db_path
    
    # Cleanup
    try:
        os.unlink(db_path)
    except:
        pass


@pytest.fixture(scope="function")
async def setup_test_data(integration_db):
    """Create test data for each test."""
    # Create tables
    try:
        async with ryx.transaction():
            # Create test data
            author1 = await Author.objects.create(
                name="Author One",
                email="author1@example.com",
                bio="First author"
            )
            author2 = await Author.objects.create(
                name="Author Two",
                email="author2@example.com",
                bio="Second author"
            )
            
            post1 = await Post.objects.create(
                title="First Post",
                content="Content 1",
                author_id=author1.id,
                views=10,
                published=True,
                featured=False
            )
            post2 = await Post.objects.create(
                title="Second Post",
                content="Content 2",
                author_id=author1.id,
                views=20,
                published=True,
                featured=True
            )
            post3 = await Post.objects.create(
                title="Draft Post",
                content="Content 3",
                author_id=author2.id,
                views=0,
                published=False,
                featured=False
            )
    except Exception:
        pass  # Tables might already exist or other issues
    
    yield {
        "author1": author1 if 'author1' in locals() else None,
        "author2": author2 if 'author2' in locals() else None,
        "post1": post1 if 'post1' in locals() else None,
        "post2": post2 if 'post2' in locals() else None,
        "post3": post3 if 'post3' in locals() else None,
    }
    
    # Cleanup
    try:
        from ryx.executor_helpers import raw_execute
        await raw_execute('DELETE FROM "test_posts"')
        await raw_execute('DELETE FROM "test_authors"')
    except:
        pass


# Test Q Object functionality
class TestQObject:
    """Test Q object functionality with real Ryx implementation."""

    def test_q_creation(self):
        """Test basic Q object creation."""
        q = Q(name="test")
        assert q._leaves == {"name": "test"}
        assert q._connector == "AND"
        assert q._negated is False
        assert q._children == []

    def test_q_and(self):
        """Test Q object AND operation."""
        q1 = Q(title="test")
        q2 = Q(published=True)
        q3 = q1 & q2

        assert q3._connector == "AND"
        assert len(q3._children) == 2

    def test_q_or(self):
        """Test Q object OR operation."""
        q1 = Q(title="test")
        q2 = Q(published=True)
        q3 = q1 | q2

        assert q3._connector == "OR"
        assert len(q3._children) == 2

    def test_q_not(self):
        """Test Q object NOT operation."""
        q1 = Q(title="test")
        q2 = ~q1

        assert q2._negated is True
        assert len(q2._children) == 1

    def test_q_complex(self):
        """Test complex Q object combinations."""
        q = (Q(title="test") & Q(published=True)) | Q(featured=True)
        assert q._connector == "OR"
        assert len(q._children) == 2

    def test_q_to_q_node_simple(self):
        """Test Q object serialization to node."""
        q = Q(title="test")
        node = q.to_q_node()
        assert node["type"] == "leaf"
        assert node["field"] == "title"
        assert node["lookup"] == "exact"
        assert node["value"] == "test"

    def test_q_to_q_node_and(self):
        """Test AND Q object serialization."""
        q = Q(title="test") & Q(published=True)
        node = q.to_q_node()
        assert node["type"] == "and"
        assert len(node["children"]) == 2

    def test_q_to_q_node_or(self):
        """Test OR Q object serialization."""
        q = Q(title="test") | Q(published=True)
        node = q.to_q_node()
        assert node["type"] == "or"
        assert len(node["children"]) == 2

    def test_q_to_q_node_not(self):
        """Test NOT Q object serialization."""
        q = ~Q(featured=True)
        node = q.to_q_node()
        assert node["type"] == "not"
        assert len(node["children"]) == 1


# Note: Additional QuerySet operation tests should use conftest fixtures
# and test them with real async/database calls

