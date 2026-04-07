"""
Unit tests for lookup parsing logic.

These tests verify the _parse_lookup_key function without requiring database.
They should NOT require any fixtures.
"""

import sys
import os

# Ensure we can import ryx
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from ryx.queryset import _parse_lookup_key


class TestLookupParsingSimple:
    """Test basic field__lookup parsing."""

    def test_exact_lookup(self):
        """Test exact lookup parsing."""
        assert _parse_lookup_key("title__exact") == ("title", "exact")
        assert _parse_lookup_key("views__exact") == ("views", "exact")

    def test_comparison_lookups(self):
        """Test comparison lookups."""
        assert _parse_lookup_key("title__gte") == ("title", "gte")
        assert _parse_lookup_key("views__lt") == ("views", "lt")
        assert _parse_lookup_key("count__lte") == ("count", "lte")

    def test_string_lookups(self):
        """Test string-specific lookups."""
        assert _parse_lookup_key("title__icontains") == ("title", "icontains")
        assert _parse_lookup_key("name__startswith") == ("name", "startswith")
        assert _parse_lookup_key("email__endswith") == ("email", "endswith")

    def test_special_lookups(self):
        """Test special lookups like isnull, in, range."""
        assert _parse_lookup_key("title__isnull") == ("title", "isnull")
        assert _parse_lookup_key("views__in") == ("views", "in")
        assert _parse_lookup_key("date__range") == ("date", "range")

    def test_simple_field_no_lookup(self):
        """Test field without lookup defaults to exact."""
        assert _parse_lookup_key("title") == ("title", "exact")
        assert _parse_lookup_key("created_at") == ("created_at", "exact")
        assert _parse_lookup_key("views") == ("views", "exact")


class TestLookupParsingDateTime:
    """Test DateTime field chained lookups."""

    def test_date_transform_only(self):
        """Test date transform without comparison (implicit exact)."""
        assert _parse_lookup_key("created_at__date") == ("created_at", "date")
        assert _parse_lookup_key("updated_at__date") == ("updated_at", "date")

    def test_year_transform_only(self):
        """Test year transform without comparison."""
        assert _parse_lookup_key("created_at__year") == ("created_at", "year")
        assert _parse_lookup_key("timestamp__year") == ("timestamp", "year")

    def test_month_transform_only(self):
        """Test month transform without comparison."""
        assert _parse_lookup_key("created_at__month") == ("created_at", "month")
        assert _parse_lookup_key("timestamp__month") == ("timestamp", "month")

    def test_day_transform_only(self):
        """Test day transform without comparison."""
        assert _parse_lookup_key("created_at__day") == ("created_at", "day")

    def test_hour_transform_only(self):
        """Test hour transform without comparison."""
        assert _parse_lookup_key("created_at__hour") == ("created_at", "hour")

    def test_minute_transform_only(self):
        """Test minute transform without comparison."""
        assert _parse_lookup_key("created_at__minute") == ("created_at", "minute")

    def test_second_transform_only(self):
        """Test second transform without comparison."""
        assert _parse_lookup_key("created_at__second") == ("created_at", "second")

    def test_week_transform_only(self):
        """Test week transform without comparison."""
        assert _parse_lookup_key("created_at__week") == ("created_at", "week")

    def test_dow_transform_only(self):
        """Test day-of-week transform without comparison."""
        assert _parse_lookup_key("created_at__dow") == ("created_at", "dow")

    def test_date_with_comparison(self):
        """Test date transform with comparison operators."""
        assert _parse_lookup_key("created_at__date__gte") == ("created_at__date", "gte")
        assert _parse_lookup_key("created_at__date__lte") == ("created_at__date", "lte")
        assert _parse_lookup_key("created_at__date__gt") == ("created_at__date", "gt")
        assert _parse_lookup_key("created_at__date__lt") == ("created_at__date", "lt")
        assert _parse_lookup_key("created_at__date__exact") == (
            "created_at__date",
            "exact",
        )

    def test_year_with_comparison(self):
        """Test year transform with comparison operators."""
        assert _parse_lookup_key("created_at__year__gte") == ("created_at__year", "gte")
        assert _parse_lookup_key("created_at__year__lt") == ("created_at__year", "lt")
        assert _parse_lookup_key("created_at__year__exact") == (
            "created_at__year",
            "exact",
        )

    def test_month_with_comparison(self):
        """Test month transform with comparison operators."""
        assert _parse_lookup_key("created_at__month__gte") == (
            "created_at__month",
            "gte",
        )
        assert _parse_lookup_key("timestamp__month__exact") == (
            "timestamp__month",
            "exact",
        )

    def test_hour_with_comparison(self):
        """Test hour transform with comparison operators."""
        assert _parse_lookup_key("created_at__hour__gte") == ("created_at__hour", "gte")
        assert _parse_lookup_key("created_at__hour__lt") == ("created_at__hour", "lt")


class TestLookupParsingJSON:
    """Test JSON field chained lookups."""

    def test_key_transform_only(self):
        """Test JSON key transform without comparison."""
        assert _parse_lookup_key("metadata__key") == ("metadata", "key")
        assert _parse_lookup_key("data__key") == ("data", "key")
        assert _parse_lookup_key("config__key") == ("config", "key")

    def test_key_text_transform(self):
        """Test JSON key text transform."""
        assert _parse_lookup_key("metadata__key_text") == ("metadata", "key_text")

    def test_json_cast_transform(self):
        """Test JSON cast transform."""
        assert _parse_lookup_key("data__json") == ("data", "json")

    def test_key_with_string_lookup(self):
        """Test JSON key with string comparison lookups."""
        assert _parse_lookup_key("metadata__key__icontains") == (
            "metadata__key",
            "icontains",
        )
        assert _parse_lookup_key("metadata__key__contains") == (
            "metadata__key",
            "contains",
        )
        assert _parse_lookup_key("metadata__key__startswith") == (
            "metadata__key",
            "startswith",
        )
        assert _parse_lookup_key("metadata__key__endswith") == (
            "metadata__key",
            "endswith",
        )
        assert _parse_lookup_key("metadata__key__exact") == ("metadata__key", "exact")

    def test_has_key_lookup(self):
        """Test has_key lookup."""
        assert _parse_lookup_key("metadata__has_key") == ("metadata", "has_key")

    def test_has_keys_lookup(self):
        """Test has_keys lookup."""
        assert _parse_lookup_key("metadata__has_keys") == ("metadata", "has_keys")

    def test_json_contains_lookup(self):
        """Test JSON contains lookup."""
        assert _parse_lookup_key("metadata__contains") == ("metadata", "contains")
        assert _parse_lookup_key("data__contains") == ("data", "contains")

    def test_json_contained_by_lookup(self):
        """Test JSON contained_by lookup."""
        assert _parse_lookup_key("metadata__contained_by") == (
            "metadata",
            "contained_by",
        )


class TestLookupParsingEdgeCases:
    """Test edge cases and mixed patterns."""

    def test_field_with_underscores(self):
        """Test field names with underscores."""
        assert _parse_lookup_key("created_at__year") == ("created_at", "year")
        assert _parse_lookup_key("user_profile__key") == ("user_profile", "key")
        assert _parse_lookup_key("my_custom_field__exact") == (
            "my_custom_field",
            "exact",
        )

    def test_multiple_transforms(self):
        """Test multiple transforms in chain."""
        # Not currently supported but should not break
        assert _parse_lookup_key("field__date__year") == ("field__date", "year")

    def test_unknown_lookup_fallback(self):
        """Test unknown lookup falls back to exact."""
        assert _parse_lookup_key("title__unknown") == ("title", "exact")
        assert _parse_lookup_key("field__foobar") == ("field", "exact")


class TestAvailableLookups:
    """Test that expected lookups are available."""

    def test_original_lookups_present(self):
        """Verify original lookups are still registered."""
        from ryx import available_lookups

        lookups = set(available_lookups())

        original = {
            "exact",
            "gt",
            "gte",
            "lt",
            "lte",
            "contains",
            "icontains",
            "startswith",
            "istartswith",
            "endswith",
            "iendswith",
            "isnull",
            "in",
            "range",
        }
        assert original.issubset(lookups), f"Missing original: {original - lookups}"

    def test_datetime_transforms_present(self):
        """Verify DateTime transforms are registered."""
        from ryx import available_lookups

        lookups = set(available_lookups())

        datetime_transforms = {
            "date",
            "year",
            "month",
            "day",
            "hour",
            "minute",
            "second",
            "week",
            "dow",
        }
        assert datetime_transforms.issubset(lookups), (
            f"Missing: {datetime_transforms - lookups}"
        )

    def test_json_lookups_present(self):
        """Verify JSON lookups are registered."""
        from ryx import available_lookups

        lookups = set(available_lookups())

        json_lookups = {
            "key",
            "key_text",
            "json",
            "has_key",
            "has_keys",
            "contains",
            "contained_by",
        }
        assert json_lookups.issubset(lookups), f"Missing: {json_lookups - lookups}"

    def test_total_lookup_count(self):
        """Verify we have expected total count."""
        from ryx import available_lookups

        lookups = available_lookups()

        # Should have at least 29 lookups
        assert len(lookups) >= 29, f"Expected >=29, got {len(lookups)}"
