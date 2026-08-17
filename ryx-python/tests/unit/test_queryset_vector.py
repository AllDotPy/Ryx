"""
Unit tests for Ryx nearest-neighbor (pgvector) QuerySet methods.

These tests verify the builder-API behaviour (op tagging and operator
validation) without touching a database. Full K-NN execution requires
PostgreSQL with the pgvector extension (see tests/integration).
"""

import sys
import types

# Mock ryx_core before importing ryx.queryset
mock_core = types.ModuleType("ryx.ryx_core")
sys.modules["ryx.ryx_core"] = mock_core

import pytest

from ryx.queryset import QuerySet


class _Meta:
    table_name = "items"


class _Model:
    _meta = _Meta()


@pytest.fixture
def qs():
    return QuerySet(_Model())


class TestOrderByDistance:
    def test_op_tag(self, qs):
        q = qs.order_by_distance("embedding", [1.0, 2.0, 3.0])
        assert ("order_by_distance", ("embedding", [1.0, 2.0, 3.0], "<->")) in q._ops

    def test_default_operator_is_l2(self, qs):
        q = qs.order_by_distance("embedding", [1.0, 2.0])
        assert q._ops[-1] == ("order_by_distance", ("embedding", [1.0, 2.0], "<->"))

    def test_cosine_operator(self, qs):
        q = qs.order_by_distance("embedding", [1.0, 2.0], operator="<=>")
        assert q._ops[-1] == ("order_by_distance", ("embedding", [1.0, 2.0], "<=>"))

    def test_inner_operator(self, qs):
        q = qs.order_by_distance("embedding", [1.0, 2.0], operator="<#>")
        assert q._ops[-1] == ("order_by_distance", ("embedding", [1.0, 2.0], "<#>"))

    def test_tuple_vector_is_normalized_to_list(self, qs):
        q = qs.order_by_distance("embedding", (1.0, 2.0))
        assert q._ops[-1][1][1] == [1.0, 2.0]

    def test_invalid_operator_raises(self, qs):
        with pytest.raises(ValueError):
            qs.order_by_distance("embedding", [1.0], operator="??")

    def test_does_not_mutate_original(self, qs):
        qs.order_by_distance("embedding", [1.0])
        assert qs._ops == []

    def test_chainable_with_limit(self, qs):
        q = qs.order_by_distance("embedding", [1.0]).limit(5)
        assert ("limit", 5) in q._ops


class TestNearestNeighbors:
    def test_op_tag(self, qs):
        q = qs.nearest_neighbors("embedding", [1.0, 2.0, 3.0], k=5)
        assert ("nearest_neighbor", ("embedding", [1.0, 2.0, 3.0], "<->", 5)) in q._ops

    def test_default_k_is_10(self, qs):
        q = qs.nearest_neighbors("embedding", [1.0, 2.0])
        assert q._ops[-1] == ("nearest_neighbor", ("embedding", [1.0, 2.0], "<->", 10))

    def test_custom_operator(self, qs):
        q = qs.nearest_neighbors("embedding", [1.0], k=3, operator="<=>")
        assert q._ops[-1] == ("nearest_neighbor", ("embedding", [1.0], "<=>", 3))

    def test_k_must_be_positive(self, qs):
        with pytest.raises(ValueError):
            qs.nearest_neighbors("embedding", [1.0], k=0)
        with pytest.raises(ValueError):
            qs.nearest_neighbors("embedding", [1.0], k=-1)

    def test_invalid_operator_raises(self, qs):
        with pytest.raises(ValueError):
            qs.nearest_neighbors("embedding", [1.0], operator="~")

    def test_does_not_mutate_original(self, qs):
        qs.nearest_neighbors("embedding", [1.0])
        assert qs._ops == []
