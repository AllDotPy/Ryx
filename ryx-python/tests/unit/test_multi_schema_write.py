"""
Unit tests for multi-schema write wiring (Python-side).

Uses the conftest mock (no DB). Verifies Meta.schema, Manager.schema(),
QuerySet.schema() propagation, and Model._resolve_schema().
"""

import pytest

from ryx import Model, AutoField, CharField


class TenantItem(Model):
    class Meta:
        table_name = "tenant_items"
        schema = "tenant1"

    id = AutoField(primary_key=True)
    name = CharField(max_length=50)


class PlainItem(Model):
    class Meta:
        table_name = "plain_items"

    id = AutoField(primary_key=True)
    name = CharField(max_length=50)


class TestMetaSchema:
    def test_meta_schema_option(self):
        assert TenantItem._meta.schema == "tenant1"

    def test_default_schema_empty(self):
        assert PlainItem._meta.schema == ""


class TestManagerSchema:
    def test_manager_schema_binds(self):
        mgr = TenantItem.objects.schema("tenant9")
        assert mgr._schema == "tenant9"
        assert mgr.get_queryset()._schema == "tenant9"

    def test_manager_using_preserves_schema(self):
        mgr = TenantItem.objects.schema("tenant2").using("logs")
        assert mgr._schema == "tenant2"
        assert mgr._alias == "logs"


class TestQuerySetSchema:
    def test_schema_sets_attr_and_op(self):
        qs = TenantItem.objects.all()
        qs2 = qs.schema("tenant3")
        assert qs2._schema == "tenant3"
        assert any(op[0] == "schema" and op[1] == "tenant3" for op in qs2._ops)

    def test_schema_does_not_mutate_original(self):
        qs = TenantItem.objects.all()
        qs.schema("tenant4")
        assert qs._schema is None


class TestResolveSchema:
    def test_resolves_from_meta(self):
        inst = TenantItem()
        assert inst._resolve_schema() == "tenant1"

    def test_instance_override_wins(self):
        inst = TenantItem()
        inst._schema = "tenant5"
        assert inst._resolve_schema() == "tenant5"

    def test_plain_returns_empty(self):
        inst = PlainItem()
        assert inst._resolve_schema() == ""


class TestHydration:
    def test_hydrate_propagates_schema(self):
        qs = TenantItem.objects.schema("tenant3").all()
        rows = [{"id": 1, "name": "x"}]
        insts = qs._hydrate(rows)
        assert insts[0]._schema == "tenant3"

    def test_hydrate_no_schema(self):
        qs = PlainItem.objects.all()
        rows = [{"id": 1, "name": "x"}]
        insts = qs._hydrate(rows)
        assert insts[0]._schema is None