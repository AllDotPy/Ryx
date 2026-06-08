"""Tests for migration state — multi-schema support."""
from __future__ import annotations

from ryx.migrations.state import (
    TableState,
    ColumnState,
    SchemaState,
    SchemaChange,
    ChangeKind,
    diff_states,
    project_state_from_models,
)


def _table(name, schema=""):
    t = TableState(name=name, schema=schema)
    t.add_column(ColumnState("id", "INTEGER", primary_key=True))
    return t


class TestDiffStates:
    def test_empty_to_table_no_schema(self):
        current = SchemaState()
        target = SchemaState()
        target.add_table(_table("posts"))
        changes = diff_states(current, target)
        assert any(c.kind == ChangeKind.CREATE_TABLE for c in changes)
        assert not any(c.kind == ChangeKind.CREATE_SCHEMA for c in changes)

    def test_empty_to_table_with_schema(self):
        current = SchemaState()
        target = SchemaState()
        target.add_table(_table("posts", schema="tenant1"))
        changes = diff_states(current, target)
        kinds = {c.kind for c in changes}
        assert ChangeKind.CREATE_SCHEMA in kinds
        assert ChangeKind.CREATE_TABLE in kinds

    def test_create_schema_before_create_table(self):
        current = SchemaState()
        target = SchemaState()
        target.add_table(_table("posts", schema="tenant1"))
        changes = diff_states(current, target)
        idx_schema = next(i for i, c in enumerate(changes) if c.kind == ChangeKind.CREATE_SCHEMA)
        idx_table = next(i for i, c in enumerate(changes) if c.kind == ChangeKind.CREATE_TABLE)
        assert idx_schema < idx_table

    def test_no_create_schema_for_empty_string(self):
        current = SchemaState()
        target = SchemaState()
        target.add_table(_table("posts", schema=""))
        changes = diff_states(current, target)
        assert not any(c.kind == ChangeKind.CREATE_SCHEMA for c in changes)

    def test_identical_with_schema_no_changes(self):
        current = SchemaState()
        current.add_table(_table("posts", schema="tenant1"))
        target = SchemaState()
        target.add_table(_table("posts", schema="tenant1"))
        assert diff_states(current, target) == []

    def test_same_table_different_schemas(self):
        current = SchemaState()
        current.add_table(_table("posts", schema="tenant1"))
        target = SchemaState()
        target.add_table(_table("posts", schema="tenant1"))
        target.add_table(_table("posts", schema="tenant2"))
        changes = diff_states(current, target)
        assert any(c.kind == ChangeKind.CREATE_SCHEMA and c.schema == "tenant2" for c in changes)
        assert any(c.kind == ChangeKind.CREATE_TABLE and c.schema == "tenant2" for c in changes)
        # tenant1.posts should have no changes (identical)
        assert not any(c.kind == ChangeKind.CREATE_TABLE and c.schema == "tenant1" for c in changes)

    def test_add_column_to_schema_table(self):
        current = SchemaState()
        current.add_table(_table("posts", schema="tenant1"))
        target = SchemaState()
        t = _table("posts", schema="tenant1")
        t.add_column(ColumnState("title", "TEXT", nullable=True))
        target.add_table(t)
        changes = diff_states(current, target)
        assert len(changes) == 1
        assert changes[0].kind == ChangeKind.ADD_COLUMN
        assert changes[0].schema == "tenant1"
        assert changes[0].column == "title"

    def test_schema_change_carries_schema(self):
        current = SchemaState()
        target = SchemaState()
        target.add_table(_table("posts", schema="blog"))
        changes = diff_states(current, target)
        ct = next(c for c in changes if c.kind == ChangeKind.CREATE_TABLE)
        assert ct.schema == "blog"

    def test_mixed_schema_and_default(self):
        current = SchemaState()
        target = SchemaState()
        target.add_table(_table("users"))  # no schema
        target.add_table(_table("posts", schema="blog"))  # with schema
        changes = diff_states(current, target)
        schema_count = sum(1 for c in changes if c.kind == ChangeKind.CREATE_SCHEMA)
        assert schema_count == 1, "Only one CreateSchema for 'blog'"
        table_count = sum(1 for c in changes if c.kind == ChangeKind.CREATE_TABLE)
        assert table_count == 2


class TestTableState:
    def test_default_schema_is_empty(self):
        t = TableState(name="posts")
        assert t.schema == ""

    def test_with_schema(self):
        t = TableState(name="posts", schema="tenant1")
        assert t.schema == "tenant1"


class TestSchemaStateSerialization:
    def test_to_json_with_schema(self):
        state = SchemaState()
        state.add_table(_table("posts", schema="tenant1"))
        data = state.to_json()
        assert '"tenant1"' in data
        assert '_schema' in data

    def test_from_json_with_schema(self):
        json_str = '{"posts": {"_schema": "tenant1", "columns": {"id": {"db_type": "INTEGER", "nullable": true, "primary_key": true, "unique": false, "default": null}}}}'
        state = SchemaState.from_json(json_str)
        assert state.tables["posts"].schema == "tenant1"

    def test_from_json_backward_compat(self):
        json_str = '{"posts": {"id": {"db_type": "INTEGER", "nullable": true, "primary_key": true, "unique": false, "default": null}}}'
        state = SchemaState.from_json(json_str)
        assert state.tables["posts"].schema == ""
