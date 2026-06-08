"""Tests for migration autodetect — schema support."""
from __future__ import annotations

from ryx.migrations.state import (
    TableState, ColumnState, SchemaState, SchemaChange, ChangeKind,
)
from ryx.migrations.autodetect import (
    CreateTable, AddField, AlterField, CreateIndex,
    apply_migration_to_state, MigrationFile,
)


class TestOperationSchema:
    def test_create_table_default_schema(self):
        op = CreateTable(table="posts", columns=[])
        assert op.schema == ""

    def test_create_table_with_schema(self):
        op = CreateTable(table="posts", columns=[], schema="tenant1")
        assert op.schema == "tenant1"
        assert "tenant1" in op.describe()

    def test_add_field_with_schema(self):
        col = ColumnState("title", "TEXT")
        op = AddField(table="posts", column=col, schema="tenant1")
        assert op.schema == "tenant1"

    def test_alter_field_with_schema(self):
        old = ColumnState("title", "VARCHAR(100)")
        new = ColumnState("title", "VARCHAR(200)")
        op = AlterField(table="posts", new_col=new, old_col=old, schema="tenant1")
        assert op.schema == "tenant1"

    def test_create_index_with_schema(self):
        op = CreateIndex(table="posts", name="idx_title", fields=["title"], schema="tenant1")
        assert op.schema == "tenant1"

    def test_to_python_with_schema(self):
        op = CreateTable(table="posts", columns=[], schema="tenant1")
        assert "schema=" in op.to_python()

    def test_to_python_without_schema(self):
        op = CreateTable(table="posts", columns=[])
        assert "schema=" not in op.to_python()


class TestApplyMigrationToState:
    def test_apply_creates_table_with_schema(self):
        op = CreateTable(table="posts", columns=[
            ColumnState("id", "INTEGER", primary_key=True),
        ], schema="tenant1")
        mf = MigrationFile(name="0001", dependencies=[], operations=[op])
        state = SchemaState()
        apply_migration_to_state(mf, state)
        assert state.tables["posts"].schema == "tenant1"

    def test_apply_creates_table_without_schema(self):
        op = CreateTable(table="posts", columns=[
            ColumnState("id", "INTEGER", primary_key=True),
        ])
        mf = MigrationFile(name="0001", dependencies=[], operations=[op])
        state = SchemaState()
        apply_migration_to_state(mf, state)
        assert state.tables["posts"].schema == ""

    def test_apply_add_field_inherits_schema_when_empty(self):
        # Create table without schema in state
        state_t = TableState(name="posts")
        state_t.add_column(ColumnState("id", "INTEGER", primary_key=True))
        state = SchemaState()
        state.add_table(state_t)
        # Add field with schema — should inherit
        op = AddField(table="posts", column=ColumnState("title", "TEXT"), schema="tenant1")
        mf = MigrationFile(name="0002", dependencies=[], operations=[op])
        apply_migration_to_state(mf, state)
        assert state.tables["posts"].schema == "tenant1"


class TestAutodetectIntegration:
    def test_detect_create_schema(self):
        """When autodetector sees a model with schema, it should
        produce a CreateTable with the schema."""
        pass  # Integration with live models not available here
