"""
Multi-schema PostgreSQL integration tests.

Requires a running PostgreSQL. Run in a subprocess to avoid
conflicts with the conftest's SQLite pool initialization.
"""

import os
import subprocess
import sys
import tempfile
import pytest

PG_URL = os.environ.get(
    "PG_TEST_URL",
    "postgres://einswilli@localhost/ryx_integration_test",
)


@pytest.fixture(scope="session")
def setup_database():
    """Override conftest's setup_database — no-op for this PG test."""
    return


@pytest.fixture(autouse=True)
def clean_tables():
    """Override conftest's async clean_tables — no-op (PG test in subprocess)."""
    return


def test_multi_schema_migration_pipeline():
    """Run the multi-schema integration test in a subprocess."""

    script = r'''
import asyncio, os, sys

PG_URL = os.environ["PG_TEST_URL"]
os.environ["RYX_AUTO_INITIALIZE"] = "0"

import ryx

async def check():
    try:
        await ryx.setup(PG_URL)
        return True
    except Exception as e:
        print("PG setup failed:", e)
        return False

if not asyncio.run(check()):
    print("PG not available")
    sys.exit(0)

from ryx import Model, CharField, IntField
from ryx.migrations import MigrationRunner

class TenantPost(Model):
    class Meta:
        table_name = "ms_posts"
    id = IntField(primary_key=True)
    title = CharField(max_length=200)

class TenantAuthor(Model):
    class Meta:
        table_name = "ms_authors"
    id = IntField(primary_key=True)
    name = CharField(max_length=100)

async def table_exists(schema, table):
    from ryx.ryx_core import raw_fetch
    rows = await raw_fetch(
        "SELECT table_name FROM information_schema.tables "
        "WHERE table_schema = '" + schema + "' AND table_name = '" + table + "'"
    )
    return len(rows) > 0

async def raw_count(schema, table):
    from ryx.ryx_core import raw_fetch
    rows = await raw_fetch(
        'SELECT count(*) AS cnt FROM "' + schema + '"."' + table + '"'
    )
    return int(rows[0]["cnt"]) if rows else 0

async def cleanup(schema):
    from ryx.ryx_core import raw_execute
    try:
        await raw_execute('DROP SCHEMA IF EXISTS "' + schema + '" CASCADE')
    except Exception:
        pass

async def main():
    # Pool already initialized by check(), no need to call setup again

    # SKIP cleanup for DB inspection
    # await cleanup("tenant1")
    # await cleanup("tenant2")

    import builtins
    original_input = builtins.input
    builtins.input = lambda prompt="": "L"

    try:
        from ryx.ryx_core import raw_execute as ddl_exec
        for s in ["tenant1", "tenant2"]:
            try:
                await ddl_exec('CREATE SCHEMA IF NOT EXISTS "' + s + '"')
            except Exception:
                pass

        # 1. Migrate tenant1
        r1 = MigrationRunner([TenantPost, TenantAuthor], schema="tenant1")
        await r1.migrate()
        assert await table_exists("tenant1", "ms_posts"), "tenant1.ms_posts should exist"
        assert await table_exists("tenant1", "ms_authors"), "tenant1.ms_authors should exist"
        assert not await table_exists("public", "ms_posts"), "should NOT be in public"

        # 2. Migrate tenant2
        r2 = MigrationRunner([TenantPost, TenantAuthor], schema="tenant2")
        await r2.migrate()
        assert await table_exists("tenant2", "ms_posts"), "tenant2.ms_posts should exist"
        assert await table_exists("tenant1", "ms_posts"), "tenant1 should still have posts"

        # 3. Insert & verify data isolation
        from ryx.ryx_core import raw_execute as dml_exec

        # Use triple-quoted SQL to avoid quoting issues
        sql1 = """INSERT INTO "tenant1"."ms_posts" (title) VALUES ('Post 1'), ('Post 2')"""
        sql2 = """INSERT INTO "tenant2"."ms_posts" (title) VALUES ('Tenant 2 Post')"""
        await dml_exec(sql1)
        await dml_exec(sql2)

        assert await raw_count("tenant1", "ms_posts") == 2, "tenant1 should have 2 posts"
        assert await raw_count("tenant2", "ms_posts") == 1, "tenant2 should have 1 post"

        # 4. Idempotent
        r3 = MigrationRunner([TenantPost, TenantAuthor], schema="tenant1")
        changes = await r3.migrate()
        assert len(changes) == 0, f"Idempotent should return 0 changes, got {changes}"
    finally:
        builtins.input = original_input

    # SKIP cleanup for DB inspection
    # await cleanup("tenant1")
    # await cleanup("tenant2")
    print("ALL CHECKS PASSED")

asyncio.run(main())
'''

    with tempfile.NamedTemporaryFile(mode="w", suffix=".py", delete=False) as f:
        f.write(script)
        script_path = f.name

    try:
        env = os.environ.copy()
        env["PG_TEST_URL"] = PG_URL

        result = subprocess.run(
            [sys.executable, script_path],
            capture_output=True,
            text=True,
            env=env,
            cwd=os.path.join(os.path.dirname(__file__), "../.."),
        )

        if result.stdout:
            print(result.stdout)
        if result.stderr:
            print(result.stderr)

        assert result.returncode == 0, f"Subprocess failed (exit={result.returncode})"
        assert "ALL CHECKS PASSED" in result.stdout, "Test did not complete successfully"
    finally:
        os.unlink(script_path)
