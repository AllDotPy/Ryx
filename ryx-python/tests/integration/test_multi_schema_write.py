"""
Multi-schema write-path integration tests (PostgreSQL).

Verifies that create()/save()/delete() and bulk_* honour the PostgreSQL
schema — the gap previously present for the write path.

Runs in a subprocess (repo pattern) to avoid conftest mock/skip setup.
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
    return


@pytest.fixture(autouse=True)
def clean_tables():
    return


def test_multi_schema_write_pipeline():
    script = r'''
import asyncio, os, sys

PG_URL = os.environ["PG_TEST_URL"]
os.environ["RYX_AUTO_INITIALIZE"] = "0"

import builtins
builtins.input = lambda prompt="": "L"

import ryx
from ryx import Model, AutoField, CharField, IntField, bulk_create, bulk_update, bulk_delete
from ryx.migrations import MigrationRunner
from ryx.ryx_core import raw_execute as ddl_exec
from ryx.ryx_core import raw_fetch as ddl_fetch

SCHEMAS = ["msa", "msb"]

class Widget(Model):
    class Meta:
        table_name = "widgets"
    id = AutoField(primary_key=True)
    name = CharField(max_length=50)
    qty = IntField(default=0)

async def count_in(schema, table="widgets"):
    rows = await ddl_fetch(
        f'SELECT count(*) AS n FROM "{schema}"."{table}"'
    )
    return rows[0]["n"]

async def main():
    try:
        await ryx.setup(PG_URL)
    except Exception as e:
        print("PG setup failed:", e)
        sys.exit(0)

    # Clean slate
    for s in SCHEMAS:
        await ddl_exec(f'DROP SCHEMA IF EXISTS "{s}" CASCADE')

    # Migrate both schemas (DDL already schema-aware)
    for s in SCHEMAS:
        await ddl_exec(f'CREATE SCHEMA IF NOT EXISTS "{s}"')
        runner = MigrationRunner([Widget], schema=s)
        await runner.migrate()

    # 1. Manager.schema().create() → INSERT into the schema
    a = await Widget.objects.schema("msa").create(name="A1", qty=1)
    assert a.pk is not None
    assert await count_in("msa") == 1, "msa should have 1 row"
    assert await count_in("msb") == 0, "msb should be empty"
    print("CREATE-IN-SCHEMA OK")

    # 2. Fetched instance remembers its schema → save() stays in msa
    a.name = "A1-updated"
    await a.save()
    assert a._schema == "msa", f"instance should remember schema, got {a._schema!r}"
    rows = await ddl_fetch('SELECT name FROM "msa"."widgets"')
    assert rows[0]["name"] == "A1-updated", "update should land in msa"
    print("SAVE-IN-SCHEMA OK")

    # 3. Meta.schema default (model declares schema)
    class BWidget(Model):
        class Meta:
            table_name = "widgets"
            schema = "msb"
        id = AutoField(primary_key=True)
        name = CharField(max_length=50)
        qty = IntField(default=0)

    b = await BWidget.objects.create(name="B1", qty=5)
    assert await count_in("msb") == 1, "BWidget create should land in msb"
    assert await count_in("msa") == 1, "msa unchanged"
    print("META-SCHEMA OK")

    # 4. bulk_create into a schema
    await bulk_create(Widget, [
        Widget(name="A2", qty=2),
        Widget(name="A3", qty=3),
    ], schema="msa")
    assert await count_in("msa") == 3, f"msa should have 3, got {await count_in('msa')}"
    print("BULK-CREATE-SCHEMA OK")

    # 5. bulk_update in schema
    fetched = await Widget.objects.schema("msa").all()
    for w in fetched:
        w.qty = 99
    await bulk_update(Widget, fetched, fields=["qty"], schema="msa")
    rows = await ddl_fetch('SELECT qty FROM "msa"."widgets"')
    assert all(r["qty"] == 99 for r in rows), f"bulk_update should apply: {rows}"
    print("BULK-UPDATE-SCHEMA OK")

    # 6. bulk_delete in schema
    await bulk_delete(Widget, fetched, schema="msa")
    assert await count_in("msa") == 0, "msa should be empty after bulk_delete"
    assert await count_in("msb") == 1, "msb untouched"
    print("BULK-DELETE-SCHEMA OK")

    # 7. delete() honours schema
    await b.delete()
    assert await count_in("msb") == 0, "msb should be empty after delete"
    print("DELETE-IN-SCHEMA OK")

    # Cleanup
    for s in SCHEMAS:
        await ddl_exec(f'DROP SCHEMA IF EXISTS "{s}" CASCADE')

    print("ALL CHECKS PASSED")

asyncio.run(main())
'''

    with tempfile.NamedTemporaryFile(mode="w", suffix=".py", delete=False) as f:
        f.write(script)
        script_path = f.name

    try:
        env = os.environ.copy()
        env["PG_TEST_URL"] = PG_URL
        project_root = os.path.abspath(
            os.path.join(os.path.dirname(__file__), "../..")
        )
        env["PYTHONPATH"] = project_root + os.pathsep + env.get("PYTHONPATH", "")
        result = subprocess.run(
            [sys.executable, script_path],
            capture_output=True,
            text=True,
            env=env,
            cwd=project_root,
        )
        if result.stdout:
            print(result.stdout)
        if result.stderr:
            print(result.stderr)
        assert result.returncode == 0, f"Subprocess failed (exit={result.returncode})"
        assert "ALL CHECKS PASSED" in result.stdout, "Test did not complete successfully"
    finally:
        os.unlink(script_path)
