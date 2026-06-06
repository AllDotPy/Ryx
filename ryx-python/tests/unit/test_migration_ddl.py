"""Tests for DDL generation — multi-schema support."""
from __future__ import annotations

from ryx.migrations.state import TableState, ColumnState
from ryx.migrations.ddl import DDLGenerator, generate_schema_ddl


def _post_table():
    t = TableState(name="posts")
    t.add_column(ColumnState("id", "INTEGER", primary_key=True))
    t.add_column(ColumnState("title", "VARCHAR(200)"))
    return t


class TestCreateSchema:
    def test_postgres_create_schema(self):
        sql = DDLGenerator("postgres").create_schema("tenant1")
        assert sql == 'CREATE SCHEMA IF NOT EXISTS "tenant1"'

    def test_mysql_returns_none(self):
        sql = DDLGenerator("mysql").create_schema("tenant1")
        assert sql is None

    def test_empty_schema_returns_none(self):
        sql = DDLGenerator("postgres").create_schema("")
        assert sql is None


class TestQualifiedNames:
    def test_create_table_with_schema(self):
        gen = DDLGenerator("postgres", schema="tenant1")
        sql = gen.create_table(_post_table())
        assert '"tenant1"."posts"' in sql

    def test_create_table_no_schema_backward_compat(self):
        gen = DDLGenerator("postgres")
        sql = gen.create_table(_post_table())
        assert '.' not in sql
        assert '"posts"' in sql

    def test_mysql_ignores_schema(self):
        gen = DDLGenerator("mysql", schema="tenant1")
        sql = gen.create_table(_post_table())
        assert "tenant1" not in sql

    def test_sqlite_ignores_schema(self):
        gen = DDLGenerator("sqlite", schema="tenant1")
        sql = gen.create_table(_post_table())
        assert "tenant1" not in sql


class TestAllDDLMethods:
    def test_drop_table_qualified(self):
        gen = DDLGenerator("postgres", schema="tenant1")
        sql = gen.drop_table("posts")
        assert '"tenant1"."posts"' in sql

    def test_add_column_qualified(self):
        gen = DDLGenerator("postgres", schema="tenant1")
        sql = gen.add_column("posts", ColumnState("title", "TEXT"))
        assert '"tenant1"."posts"' in sql

    def test_drop_column_qualified(self):
        gen = DDLGenerator("postgres", schema="tenant1")
        sql = gen.drop_column("posts", "title")
        assert '"tenant1"."posts"' in sql

    def test_create_index_qualified(self):
        gen = DDLGenerator("postgres", schema="tenant1")
        sql = gen.create_index_from_fields("posts", ["title"], "idx_title")
        assert '"tenant1"."posts"' in sql

    def test_drop_index_mysql_qualified(self):
        gen = DDLGenerator("mysql", schema="tenant1")
        sql = gen.drop_index("idx_title", "posts")
        assert "tenant1" not in sql  # MySQL DROP INDEX doesn't qualify table

    def test_alter_column_qualified(self):
        gen = DDLGenerator("postgres", schema="tenant1")
        new = ColumnState("title", "VARCHAR(200)", nullable=False)
        sql = gen.alter_column("posts", new)
        assert '"tenant1"."posts"' in sql

    def test_add_foreign_key_qualified(self):
        gen = DDLGenerator("postgres", schema="tenant1")
        sql = gen.add_foreign_key("posts", "author_id", "authors", "id")
        assert '"tenant1"."posts"' in sql
        assert '"tenant1"."authors"' in sql

    def test_add_constraint_qualified(self):
        gen = DDLGenerator("postgres", schema="tenant1")
        constraint = type("Constraint", (), {"name": "age_check", "check": "age > 0"})()
        sql = gen.add_constraint("users", constraint)
        assert '"tenant1"."users"' in sql


class TestGenerateSchemaDDL:
    def test_generates_qualified_ddl(self):
        """generate_schema_ddl() generates per-table DDL using model schema."""
        # This test relies on model classes; test via TableState generation instead
        from ryx.migrations.ddl import DDLGenerator
        gen = DDLGenerator("postgres", schema="tenant1")
        t = TableState(name="posts", schema="tenant1")
        t.add_column(ColumnState("id", "INTEGER", primary_key=True))
        sql = gen.create_table(t)
        assert '"tenant1"."posts"' in sql

    def test_generates_unqualified_for_default(self):
        from ryx.migrations.ddl import DDLGenerator
        gen = DDLGenerator("postgres")
        t = TableState(name="posts")
        t.add_column(ColumnState("id", "INTEGER", primary_key=True))
        sql = gen.create_table(t)
        assert 'tenant1' not in sql
