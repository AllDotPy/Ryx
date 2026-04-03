"""
Integration tests for query operations.
"""

import pytest
from conftest import Author, Post, Tag, Q


class TestBasicFilters:
    """Test basic filter operations."""

    @pytest.mark.asyncio
    async def test_filter_exact(self, clean_tables):
        """Test exact match filtering."""
        await Post.objects.create(title="Python Guide", views=10)
        await Post.objects.create(title="Rust Guide", views=20)
        await Post.objects.create(title="Django Tips", views=30)

        results = await Post.objects.filter(title="Python Guide")
        assert len(results) == 1
        assert results[0].title == "Python Guide"

    @pytest.mark.asyncio
    async def test_filter_icontains(self, clean_tables):
        """Test case-insensitive contains filtering."""
        await Post.objects.create(title="Python Tutorial")
        await Post.objects.create(title="RUST Tutorial")
        await Post.objects.create(title="Django Guide")

        results = await Post.objects.filter(title__icontains="tutorial")
        assert len(results) == 2

    @pytest.mark.asyncio
    async def test_filter_startswith(self, clean_tables):
        """Test startswith filtering."""
        await Post.objects.create(title="Python Basics")
        await Post.objects.create(title="Python Advanced")
        await Post.objects.create(title="Rust Guide")

        results = await Post.objects.filter(title__startswith="Python")
        assert len(results) == 2

    @pytest.mark.asyncio
    async def test_filter_gte_lte(self, clean_tables):
        """Test greater than or equal and less than or equal."""
        await Post.objects.create(title="Post 1", views=10)
        await Post.objects.create(title="Post 2", views=20)
        await Post.objects.create(title="Post 3", views=30)
        await Post.objects.create(title="Post 4", views=40)

        results = await Post.objects.filter(views__gte=20, views__lte=35)
        assert len(results) == 2
        views = sorted([r.views for r in results])
        assert views == [20, 30]

    @pytest.mark.asyncio
    async def test_filter_in(self, clean_tables):
        """Test in filtering."""
        p1 = await Post.objects.create(title="Post 1", views=10)
        p2 = await Post.objects.create(title="Post 2", views=20)
        p3 = await Post.objects.create(title="Post 3", views=30)

        results = await Post.objects.filter(id__in=[p1.pk, p3.pk])
        assert len(results) == 2
        titles = {r.title for r in results}
        assert titles == {"Post 1", "Post 3"}

    @pytest.mark.asyncio
    async def test_filter_isnull(self, clean_tables):
        """Test isnull filtering."""
        await Post.objects.create(title="With Body", body="Content")
        await Post.objects.create(title="No Body")

        results = await Post.objects.filter(body__isnull=True)
        assert len(results) == 1
        assert results[0].title == "No Body"

        results = await Post.objects.filter(body__isnull=False)
        assert len(results) == 1
        assert results[0].title == "With Body"

    @pytest.mark.asyncio
    async def test_filter_range(self, clean_tables):
        """Test range filtering."""
        for views in [5, 15, 25, 35, 45]:
            await Post.objects.create(title=f"Post {views}", views=views)

        results = await Post.objects.filter(views__range=(10, 40))
        assert len(results) == 3
        views = sorted([r.views for r in results])
        assert views == [15, 25, 35]


class TestExclude:
    """Test exclude operations."""

    @pytest.mark.asyncio
    async def test_exclude_simple(self, clean_tables):
        """Test basic exclude."""
        await Post.objects.create(title="Draft", active=False)
        await Post.objects.create(title="Published 1", active=True)
        await Post.objects.create(title="Published 2", active=True)

        results = await Post.objects.exclude(active=False)
        assert len(results) == 2
        assert all(r.active for r in results)

    @pytest.mark.asyncio
    async def test_exclude_with_filter(self, clean_tables):
        """Test exclude combined with filter."""
        await Post.objects.create(title="Python", views=100, active=True)
        await Post.objects.create(title="Rust", views=50, active=True)
        await Post.objects.create(title="Draft", views=10, active=False)

        results = await Post.objects.filter(views__gte=20).exclude(active=False)
        assert len(results) == 2


class TestQObjects:
    """Test Q object operations."""

    @pytest.mark.asyncio
    async def test_q_or(self, clean_tables):
        """Test Q object OR operation."""
        await Post.objects.create(title="Featured", views=5, active=False)
        await Post.objects.create(title="Popular", views=1000, active=False)
        await Post.objects.create(title="Normal", views=5, active=True)

        results = await Post.objects.filter(Q(active=True) | Q(views__gte=1000))
        assert len(results) == 2

    @pytest.mark.asyncio
    async def test_q_and(self, clean_tables):
        """Test Q object AND operation."""
        await Post.objects.create(title="Python", views=100, active=True)
        await Post.objects.create(title="Rust", views=10, active=True)
        await Post.objects.create(title="Draft", views=100, active=False)

        results = await Post.objects.filter(Q(views__gte=50) & Q(active=True))
        assert len(results) == 1
        assert results[0].title == "Python"

    @pytest.mark.asyncio
    async def test_q_not(self, clean_tables):
        """Test Q object NOT operation."""
        await Post.objects.create(title="Draft", active=False)
        await Post.objects.create(title="Published", active=True)

        results = await Post.objects.filter(~Q(active=False))
        assert len(results) == 1
        assert results[0].title == "Published"

    @pytest.mark.asyncio
    async def test_q_complex(self, clean_tables):
        """Test complex Q object combinations."""
        await Post.objects.create(title="Featured Python", views=100, active=True)
        await Post.objects.create(title="Draft Python", views=50, active=False)
        await Post.objects.create(title="Featured Rust", views=10, active=True)
        await Post.objects.create(title="Normal", views=5, active=True)

        # (active=True AND views >= 50) OR title__icontains="Featured"
        results = await Post.objects.filter(
            (Q(active=True) & Q(views__gte=50)) | Q(title__icontains="Featured")
        )
        assert len(results) == 2

    @pytest.mark.asyncio
    async def test_q_mixed_with_kwargs(self, clean_tables):
        """Test Q objects mixed with regular filter kwargs."""
        await Post.objects.create(title="Python", views=100, active=True)
        await Post.objects.create(title="Rust", views=30, active=True)
        await Post.objects.create(title="Draft", views=100, active=False)

        results = await Post.objects.filter(
            Q(views__gte=50) | Q(views__lte=25), active=True
        )
        assert len(results) == 1
        assert results[0].title == "Python"


class TestOrdering:
    """Test ordering operations."""

    @pytest.mark.asyncio
    async def test_order_by_single_field(self, clean_tables):
        """Test ordering by a single field."""
        await Post.objects.create(title="Z Post", views=10)
        await Post.objects.create(title="A Post", views=20)
        await Post.objects.create(title="M Post", views=30)

        results = await Post.objects.order_by("title")
        assert len(results) == 3
        assert results[0].title == "A Post"
        assert results[1].title == "M Post"
        assert results[2].title == "Z Post"

    @pytest.mark.asyncio
    async def test_order_by_descending(self, clean_tables):
        """Test descending order."""
        await Post.objects.create(title="Z Post", views=10)
        await Post.objects.create(title="A Post", views=20)

        results = await Post.objects.order_by("-title")
        assert results[0].title == "Z Post"
        assert results[1].title == "A Post"

    @pytest.mark.asyncio
    async def test_order_by_multiple_fields(self, clean_tables):
        """Test ordering by multiple fields."""
        await Post.objects.create(title="A Post", views=30)
        await Post.objects.create(title="A Post", views=10)
        await Post.objects.create(title="B Post", views=20)

        results = await Post.objects.order_by("title", "-views")
        assert results[0].title == "A Post" and results[0].views == 30
        assert results[1].title == "A Post" and results[1].views == 10
        assert results[2].title == "B Post" and results[2].views == 20


class TestPagination:
    """Test pagination operations."""

    @pytest.mark.asyncio
    async def test_limit(self, clean_tables):
        """Test limiting results."""
        for i in range(5):
            await Post.objects.create(title=f"Post {i}", views=i)

        results = await Post.objects.order_by("views")[:3]
        assert len(results) == 3
        assert [r.views for r in results] == [0, 1, 2]

    @pytest.mark.asyncio
    async def test_offset(self, clean_tables):
        """Test offsetting results."""
        for i in range(5):
            await Post.objects.create(title=f"Post {i}", views=i)

        results = await Post.objects.order_by("views")[2:5]
        assert len(results) == 3
        assert [r.views for r in results] == [2, 3, 4]

    @pytest.mark.asyncio
    async def test_limit_offset(self, clean_tables):
        """Test both limit and offset."""
        for i in range(10):
            await Post.objects.create(title=f"Post {i}", views=i)

        results = await Post.objects.order_by("views")[3:7]
        assert len(results) == 4
        assert [r.views for r in results] == [3, 4, 5, 6]


class TestDistinct:
    """Test distinct operations."""

    @pytest.mark.asyncio
    async def test_distinct(self, clean_tables):
        """Test distinct results."""
        # Create posts with duplicate titles
        await Post.objects.create(title="Same Title", views=10)
        await Post.objects.create(title="Same Title", views=20)
        await Post.objects.create(title="Different Title", views=30)

        # Without distinct
        all_results = await Post.objects.filter(title="Same Title")
        assert len(all_results) == 2

        # With distinct (on title)
        distinct_results = await Post.objects.filter(title="Same Title").distinct()
        # Note: distinct() affects the SQL query, but since we're filtering by title,
        # all results already have the same title
        assert len(distinct_results) == 2


class TestChaining:
    """Test query chaining."""

    @pytest.mark.asyncio
    async def test_complex_chaining(self, clean_tables):
        """Test complex query chaining."""
        await Post.objects.create(title="Python Guide", views=100, active=True)
        await Post.objects.create(title="Rust Guide", views=50, active=True)
        await Post.objects.create(title="Draft Guide", views=75, active=False)
        await Post.objects.create(title="Old Post", views=25, active=True)

        results = await (
            Post.objects.filter(views__gte=30)
            .exclude(title__startswith="Draft")
            .order_by("-views")
            .filter(active=True)
        )

        assert len(results) == 2
        assert results[0].title == "Python Guide"
        assert results[1].title == "Rust Guide"
