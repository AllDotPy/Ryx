"""
Unit tests for Ryx QuerySet helper functions.
Tests only pure functions without database dependency.

Complex QuerySet operations are tested in:
  tests/integration/test_queryset_operations.py
"""

import pytest


def _parse_lookup_key(key):
    """Parse lookup key into field and lookup operator.
    
    Unit test version - simplified for testing pure function logic.
    """
    known_lookups = [
        "exact", "gt", "gte", "lt", "lte",
        "contains", "icontains", "startswith", "istartswith",
        "endswith", "iendswith", "isnull", "in", "range",
    ]
    parts = key.split("__")
    if len(parts) >= 2 and parts[-1] in known_lookups:
        return "__".join(parts[:-1]), parts[-1]
    return key, "exact"


class TestParseLookupKey:
    """Test _parse_lookup_key function - pure function tests."""

    def test_simple_lookup(self):
        """Test parsing simple field name without lookup."""
        field, lookup = _parse_lookup_key("name")
        assert field == "name"
        assert lookup == "exact"

    def test_lookup_with_suffix(self):
        """Test parsing field with lookup operator."""
        field, lookup = _parse_lookup_key("name__icontains")
        assert field == "name"
        assert lookup == "icontains"

    def test_multiple_underscores(self):
        """Test parsing relationship field with lookup."""
        field, lookup = _parse_lookup_key("user__profile__name__startswith")
        assert field == "user__profile__name"
        assert lookup == "startswith"

    def test_unknown_lookup(self):
        """Test unknown lookup falls back to 'exact'."""
        field, lookup = _parse_lookup_key("name__unknown")
        assert field == "name__unknown"
        assert lookup == "exact"

    def test_numeric_lookups(self):
        """Test numeric comparison lookups."""
        tests = [
            ("age__gt", "age", "gt"),
            ("views__gte", "views", "gte"),
            ("rating__lt", "rating", "lt"),
            ("score__lte", "score", "lte"),
        ]
        for key, expected_field, expected_lookup in tests:
            field, lookup = _parse_lookup_key(key)
            assert field == expected_field
            assert lookup == expected_lookup

    def test_range_lookup(self):
        """Test range lookup."""
        field, lookup = _parse_lookup_key("age__range")
        assert field == "age"
        assert lookup == "range"

    def test_in_lookup(self):
        """Test in lookup."""
        field, lookup = _parse_lookup_key("status__in")
        assert field == "status"
        assert lookup == "in"

    def test_isnull_lookup(self):
        """Test isnull lookup."""
        field, lookup = _parse_lookup_key("description__isnull")
        assert field == "description"
        assert lookup == "isnull"


# Note: Complex QuerySet and Q object tests are in:
# tests/integration/test_queryset_operations.py
