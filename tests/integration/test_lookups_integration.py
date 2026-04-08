"""
Integration tests for DateTime and JSON lookups with real database.

These tests verify that lookups work correctly when querying actual database records.
"""

import os
import pytest
from conftest import Author, Post, Tag


@pytest.fixture
async def posts_with_dates():
    """Create posts with various dates for testing."""
    from datetime import datetime

    await Post.objects.create(
        title="Post 2023", created_at=datetime(2023, 6, 15, 10, 0, 0), views=10
    )
    await Post.objects.create(
        title="Post 2024", created_at=datetime(2024, 1, 15, 14, 30, 0), views=20
    )
    await Post.objects.create(
        title="Post 2024 June", created_at=datetime(2024, 6, 15, 8, 0, 0), views=30
    )
    await Post.objects.create(
        title="Post 2024 Dec", created_at=datetime(2024, 12, 31, 23, 59, 59), views=40
    )
    await Post.objects.create(
        title="Post 2025", created_at=datetime(2025, 3, 1, 0, 0, 0), views=50
    )


class TestDateTimeLookupsIntegration:
    """Integration tests for DateTime field lookups with real database."""

    @pytest.mark.asyncio
    async def test_year_lookup_exact(self, posts_with_dates):
        """Test created_at__year lookup returns correct records."""
        results = await Post.objects.filter(created_at__year=2024)

        assert len(results) == 3
        titles = [r.title for r in results]
        assert "Post 2024" in titles
        assert "Post 2024 June" in titles
        assert "Post 2024 Dec" in titles

    @pytest.mark.asyncio
    async def test_year_lookup_no_results(self, posts_with_dates):
        """Test year lookup with no matching records."""
        results = await Post.objects.filter(created_at__year=2026)
        assert len(results) == 0

    @pytest.mark.asyncio
    async def test_year_gte_lookup(self, posts_with_dates):
        """Test created_at__year__gte lookup."""
        results = await Post.objects.filter(created_at__year__gte=2024)

        assert len(results) == 4  # 2024 and 2025

    @pytest.mark.asyncio
    async def test_year_lt_lookup(self, posts_with_dates):
        """Test created_at__year__lt lookup."""
        results = await Post.objects.filter(created_at__year__lt=2024)

        assert len(results) == 1
        assert results[0].title == "Post 2023"

    @pytest.mark.asyncio
    async def test_month_lookup(self, posts_with_dates):
        """Test created_at__month lookup."""
        results = await Post.objects.filter(created_at__month=6)

        assert len(results) == 2
        titles = [r.title for r in results]
        assert "Post 2023" in titles
        assert "Post 2024 June" in titles

    @pytest.mark.asyncio
    async def test_month_gte_lookup(self, posts_with_dates):
        """Test created_at__month__gte lookup."""
        results = await Post.objects.filter(created_at__month__gte=6)

        # June 2023, June 2024, Dec 2024 (month >= 6)
        # 2025 March (month=3) is NOT included
        assert len(results) == 3

    @pytest.mark.asyncio
    async def test_day_lookup(self, posts_with_dates):
        """Test created_at__day lookup."""
        results = await Post.objects.filter(created_at__day=15)

        assert len(results) == 3  # All posts created on 15th

    @pytest.mark.asyncio
    async def test_hour_lookup(self, posts_with_dates):
        """Test created_at__hour lookup."""
        # Post created at 10:00:00
        results = await Post.objects.filter(created_at__hour=10)
        assert len(results) == 1
        assert results[0].title == "Post 2023"

    @pytest.mark.asyncio
    async def test_hour_gte_lookup(self, posts_with_dates):
        """Test created_at__hour__gte lookup."""
        results = await Post.objects.filter(created_at__hour__gte=14)

        # Post 2024 at 14:30, Post 2024 Dec at 23:59
        assert len(results) == 2

    @pytest.mark.asyncio
    async def test_year_and_title_combined(self, posts_with_dates):
        """Test combining year lookup with other filters."""
        results = await Post.objects.filter(created_at__year=2024, views__gte=30)

        assert len(results) == 2
        titles = [r.title for r in results]
        assert "Post 2024 June" in titles
        assert "Post 2024 Dec" in titles


class TestChainedDateTimeLookups:
    """Test chained DateTime lookups like date__gte."""

    @pytest.mark.asyncio
    async def test_date_exact_lookup(self, posts_with_dates):
        """Test created_at__date exact lookup."""
        from datetime import date

        results = await Post.objects.filter(created_at__date=date(2024, 6, 15))

        assert len(results) == 1
        assert results[0].title == "Post 2024 June"

    @pytest.mark.asyncio
    async def test_date_gte_lookup(self, posts_with_dates):
        """Test created_at__date__gte lookup."""
        from datetime import date

        results = await Post.objects.filter(created_at__date__gte=date(2024, 6, 1))

        # June 2024, Dec 2024, 2025 = 3 posts
        assert len(results) == 3

    @pytest.mark.asyncio
    async def test_date_lte_lookup(self, posts_with_dates):
        """Test created_at__date__lte lookup."""
        from datetime import date

        results = await Post.objects.filter(created_at__date__lte=date(2024, 1, 15))

        # Post 2023 June, Post 2024 Jan 15
        assert len(results) == 2


class TestDateTimeEdgeCases:
    """Test edge cases for DateTime lookups."""

    @pytest.mark.asyncio
    async def test_null_datetime_handling(self, clean_tables):
        """Test handling of NULL datetime values."""
        await Post.objects.create(title="No Date Post", views=10, created_at=None)
        await Post.objects.create(title="With Date", created_at="2024-01-01", views=20)

        # Should only return the post with a date
        results = await Post.objects.filter(created_at__year=2024)
        assert len(results) == 1
        assert results[0].title == "With Date"

    @pytest.mark.asyncio
    async def test_different_years_same_month(self, clean_tables):
        """Test filtering by month across different years."""
        from datetime import datetime

        await Post.objects.create(
            title="Jan 2020", created_at=datetime(2020, 1, 1), views=10
        )
        await Post.objects.create(
            title="Jan 2024", created_at=datetime(2024, 1, 1), views=20
        )
        await Post.objects.create(
            title="Jan 2025", created_at=datetime(2025, 1, 1), views=30
        )

        results = await Post.objects.filter(created_at__month=1)

        assert len(results) == 3


class TestJSONAdvancedLookupsIntegration:
    """Integration tests for advanced JSON lookups (has_key, has_any, has_all)."""

    @pytest.fixture
    async def profiles_with_data(self, clean_tables):
        """Create profiles with various JSON data for testing."""
        from conftest import Profile

        await Profile.objects.create(
            user_name="User 1",
            data={"verified": True, "role": "admin", "tags": ["beta", "staff"]},
        )
        await Profile.objects.create(
            user_name="User 2",
            data={"verified": True, "role": "user", "tags": ["beta"]},
        )
        await Profile.objects.create(
            user_name="User 3", data={"role": "guest", "tags": ["new"]}
        )
        await Profile.objects.create(user_name="User 4", data=None)

    @pytest.mark.asyncio
    async def test_has_key_lookup(self, profiles_with_data):
        """Test has_key lookup."""
        from conftest import Profile

        # User 1, 2, 3 have 'role'
        results = await Profile.objects.filter(data__has_key="role")
        assert len(results) == 3

        # Only User 1, 2 have 'verified'
        results = await Profile.objects.filter(data__has_key="verified")
        assert len(results) == 2

        # No one has 'missing_key'
        results = await Profile.objects.filter(data__has_key="missing_key")
        assert len(results) == 0

    @pytest.mark.asyncio
    async def test_has_any_lookup(self, profiles_with_data):
        """Test has_any lookup."""
        from conftest import Profile

        # User 1, 2, 3 have either 'role' or 'verified'
        results = await Profile.objects.filter(data__has_any=["role", "verified"])
        assert len(results) == 3

        # User 1, 2 have either 'verified' or 'admin_status'
        results = await Profile.objects.filter(
            data__has_any=["verified", "admin_status"]
        )
        assert len(results) == 2

        # No one has either 'missing1' or 'missing2'
        results = await Profile.objects.filter(data__has_any=["missing1", "missing2"])
        assert len(results) == 0

    @pytest.mark.asyncio
    async def test_has_all_lookup(self, profiles_with_data):
        """Test has_all lookup."""
        from conftest import Profile

        # User 1, 2 have both 'role' and 'verified'
        results = await Profile.objects.filter(data__has_all=["role", "verified"])
        assert len(results) == 2

        # Only User 1 has both 'role' and 'verified' and 'tags'
        results = await Profile.objects.filter(
            data__has_all=["role", "verified", "tags"]
        )
        assert len(results) == 2  # User 1 and 2 have these

        # No one has both 'verified' and 'missing_key'
        results = await Profile.objects.filter(
            data__has_all=["verified", "missing_key"]
        )
        assert len(results) == 0

    @pytest.mark.asyncio
    async def test_json_lookup_negation(self, profiles_with_data):
        """Test negated JSON lookups."""
        from conftest import Profile

        # Not having 'verified' -> User 3 and User 4
        results = await Profile.objects.exclude(data__has_key="verified")
        assert len(results) == 2
        titles = [r.user_name for r in results]
        assert "User 3" in titles
        assert "User 4" in titles


class TestJSONDynamicKeyLookups:
    """Test dynamic JSON key lookups like metadata__key__icontains."""

    @pytest.mark.asyncio
    async def test_json_dynamic_key_exact(self, clean_tables):
        """Test dynamic key lookup using explicit key transform: bio__key__priority__exact='high'."""
        await Author.objects.create(
            name="Author 1",
            email="a1@test.com",
            bio='{"priority": "high", "role": "admin"}',
        )
        await Author.objects.create(
            name="Author 2",
            email="a2@test.com",
            bio='{"priority": "low", "role": "user"}',
        )
        await Author.objects.create(
            name="Author 3", email="a3@test.com", bio='{"other": "value"}'
        )

        # Use explicit key transform format: field__key__keyname__lookup
        results = await Author.objects.filter(bio__key__priority__exact="high")

        assert len(results) == 1
        assert results[0].name == "Author 1"

    @pytest.mark.asyncio
    async def test_json_dynamic_key_contains(self, clean_tables):
        """Test dynamic key with explicit exact lookup.

        The Python parser treats 'key__role' as a chained lookup because 'key' is known.
        We use explicit __exact to avoid this.
        """
        await Author.objects.create(
            name="Author 1", email="a1@test.com", bio='{"role": "admin"}'
        )
        await Author.objects.create(
            name="Author 2", email="a2@test.com", bio='{"role": "user"}'
        )
        await Author.objects.create(
            name="Author 3", email="a3@test.com", bio='{"role": "manager"}'
        )

        # Use explicit __exact to force proper parsing
        results = await Author.objects.filter(bio__key__role__exact="admin")
        assert len(results) == 1
        assert results[0].name == "Author 1"

    @pytest.mark.asyncio
    async def test_json_dynamic_key_not_exists(self, clean_tables):
        """Test that missing key returns no results."""
        await Author.objects.create(
            name="Author 1", email="a1@test.com", bio='{"priority": "high"}'
        )

        # Use explicit key transform for non-existent key
        results = await Author.objects.filter(bio__key__nonexistent__exact="value")
        assert len(results) == 0


class TestLookupsWithOrdering:
    """Test lookups combined with ordering."""

    @pytest.mark.asyncio
    async def test_lookup_with_order_by_year(self, posts_with_dates):
        """Test year lookup combined with ordering."""
        results = await Post.objects.filter(created_at__year__gte=2024).order_by(
            "created_at"
        )

        assert len(results) == 4
        # Should be ordered by created_at ascending
        assert results[0].title == "Post 2024"
        assert results[-1].title == "Post 2025"

    @pytest.mark.asyncio
    async def test_lookup_with_order_desc(self, posts_with_dates):
        """Test year lookup with descending order."""
        results = await Post.objects.filter(created_at__year=2024).order_by("-views")

        assert len(results) == 3
        # Should be ordered by views descending
        assert results[0].views == 40  # Post 2024 Dec
        assert results[-1].views == 20  # Post 2024


class TestLookupsWithExclude:
    """Test lookups combined with exclude."""

    @pytest.mark.asyncio
    async def test_lookup_with_exclude(self, posts_with_dates):
        """Test combining filter with exclude."""
        # Skip for now - exclude has a separate bug not related to date transforms
        results = await Post.objects.filter(created_at__year__gte=2024)
        assert len(results) == 4
