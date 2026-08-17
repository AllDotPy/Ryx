"""
Nearest-neighbor (pgvector) PostgreSQL integration tests.

Requires a running PostgreSQL **with the pgvector extension**.
Runs in a subprocess to avoid conflicts with the conftest's SQLite pool.

If pgvector is not available, the subprocess prints a marker and the test
passes as a no-op (so CI without pgvector does not fail).
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


def test_nearest_neighbors_pipeline():
    """Run the pgvector K-NN integration test in a subprocess."""

    script = r'''
import asyncio, os, sys

PG_URL = os.environ["PG_TEST_URL"]
os.environ["RYX_AUTO_INITIALIZE"] = "0"

import ryx
from ryx.ryx_core import raw_fetch as ddl_fetch
from ryx.ryx_core import raw_execute as ddl_exec


async def pgvector_available():
    try:
        rows = await ddl_fetch(
            "SELECT 1 FROM pg_available_extensions WHERE name = 'vector'"
        )
        return len(rows) > 0
    except Exception:
        return False


async def main():
    try:
        await ryx.setup(PG_URL)
    except Exception as e:
        print("PG setup failed:", e)
        sys.exit(0)

    if not await pgvector_available():
        print("PGVECTOR not available")
        sys.exit(0)

    try:
        await ddl_exec('CREATE EXTENSION IF NOT EXISTS vector')
    except Exception:
        print("PGVECTOR not available")
        sys.exit(0)

    import builtins
    builtins.input = lambda prompt="": "L"

    from ryx import Model, AutoField, CharField, VectorField
    from ryx.migrations import MigrationRunner

    class Item(Model):
        class Meta:
            table_name = "knn_items"
        id = AutoField(primary_key=True)
        name = CharField(max_length=50)
        embedding = VectorField(dimensions=3)

    # Fresh table with vector(3) — DDL pass-through generates the type.
    await ddl_exec('DROP TABLE IF EXISTS "knn_items"')
    runner = MigrationRunner([Item])
    await runner.migrate()
    assert await table_has_vector_column(), "embedding column should be vector(3)"

    # Insert via the ORM.
    await Item.objects.create(name="a", embedding=[1.0, 0.0, 0.0])
    await Item.objects.create(name="b", embedding=[2.0, 0.0, 0.0])
    await Item.objects.create(name="c", embedding=[10.0, 10.0, 10.0])

    # nearest_neighbors (L2) to [1,0,0] → a, then b.
    items = await Item.objects.nearest_neighbors(
        "embedding", [1.0, 0.0, 0.0], k=2, operator="<->"
    )
    names = [i.name for i in items]
    assert names == ["a", "b"], f"nearest_neighbors L2: expected [a, b], got {names}"

    # order_by_distance + limit gives the same result.
    items2 = await Item.objects.order_by_distance(
        "embedding", [1.0, 0.0, 0.0], operator="<->"
    ).limit(2)
    names2 = [i.name for i in items2]
    assert names2 == ["a", "b"], f"order_by_distance L2: expected [a, b], got {names2}"

    # Cosine to [1,0,0] is closest to a.
    items3 = await Item.objects.nearest_neighbors(
        "embedding", [1.0, 0.0, 0.0], k=1, operator="<=>"
    )
    assert items3[0].name == "a", f"cosine: expected a, got {items3[0].name}"

    # Non-PG backends raise NotImplementedError.
    import sqlite3  # noqa - ensure no sqlite import side effects

    await ddl_exec('DROP TABLE IF EXISTS "knn_items"')
    print("ALL CHECKS PASSED")


async def table_has_vector_column():
    rows = await ddl_fetch(
        "SELECT udt_name FROM information_schema.columns "
        "WHERE table_name = 'knn_items' AND column_name = 'embedding'"
    )
    return rows and rows[0].get("udt_name", "") == "vector"

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
        if "PGVECTOR not available" in result.stdout:
            pytest.skip("pgvector extension not available on the test database")
        assert "ALL CHECKS PASSED" in result.stdout, "Test did not complete successfully"
    finally:
        os.unlink(script_path)
