"""
End-to-end file storage integration tests (real filesystem + SQLite).

Runs the pipeline in a subprocess (the repo's established pattern for
integration tests) to avoid the conftest's mock/skip setup.
"""

import os
import subprocess
import sys
import tempfile

import pytest


@pytest.fixture(scope="session")
def setup_database():
    """Override conftest's setup_database — handled inside the subprocess."""
    return


@pytest.fixture(autouse=True)
def clean_tables():
    """Override conftest's async clean_tables — no-op (subprocess)."""
    return


def test_file_storage_pipeline():
    script = r'''
import asyncio, os, sys, tempfile

os.environ["RYX_AUTO_INITIALIZE"] = "0"

import ryx
from ryx import (
    Model, AutoField, CharField, FileField, ImageField,
    FileSystemStorage, configure_storage,
)
from ryx.storage import clear_storage

PNG_BYTES = b"\x89PNG\r\n\x1a\n" + b"\x00" * 64

class Doc(Model):
    class Meta:
        table_name = "e2e_docs"
    id = AutoField(primary_key=True)
    title = CharField(max_length=50)
    attachment = FileField(upload_to="docs/")

class Img(Model):
    class Meta:
        table_name = "e2e_imgs"
    id = AutoField(primary_key=True)
    name = CharField(max_length=50)
    image = ImageField(upload_to="images/")

async def main():
    media = tempfile.mkdtemp(prefix="ryx_e2e_media_")
    db_path = os.path.join(tempfile.gettempdir(), "ryx_e2e_files.sqlite3")
    if os.path.exists(db_path):
        os.remove(db_path)

    await ryx.setup(f"sqlite:///{db_path}?mode=rwc")
    configure_storage(FileSystemStorage(root=media, base_url="/media/"))

    from ryx.executor_helpers import raw_execute
    await raw_execute('DROP TABLE IF EXISTS "e2e_docs"')
    await raw_execute(
        'CREATE TABLE "e2e_docs" ('
        "id INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT, attachment VARCHAR(255))"
    )
    await raw_execute('DROP TABLE IF EXISTS "e2e_imgs"')
    await raw_execute(
        'CREATE TABLE "e2e_imgs" ('
        "id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT, image VARCHAR(255))"
    )

    # 1. FileField staged bytes → committed to disk
    doc = await Doc.objects.create(title="hello", attachment=b"HELLO WORLD")
    assert doc.attachment is not None, "attachment should be a FieldFile"
    assert doc.attachment.name.startswith("docs/"), doc.attachment.name
    path = os.path.join(media, doc.attachment.name)
    assert os.path.exists(path), "file should exist on disk"
    with open(path, "rb") as fh:
        assert fh.read() == b"HELLO WORLD"
    assert doc.attachment.url == f"/media/{doc.attachment.name}"
    print("FILE FIELD OK:", doc.attachment.name)

    # 2. Reload from DB
    doc2 = await Doc.objects.get(id=doc.id)
    assert doc2.attachment.name == doc.attachment.name
    assert await doc2.attachment.read() == b"HELLO WORLD"
    assert await doc2.attachment.exists()
    print("RELOAD OK")

    # 3. Explicit FieldFile.save
    from ryx.files import FieldFile
    ff = FieldFile(doc, Doc._meta.fields["attachment"], None)
    await ff.save("docs/explicit.txt", b"EXPLICIT")
    assert open(os.path.join(media, "docs/explicit.txt"), "rb").read() == b"EXPLICIT"
    print("EXPLICIT SAVE OK")

    # 4. Delete removes the file
    await doc2.delete()
    assert not os.path.exists(path), "file should be deleted"
    print("DELETE CLEANUP OK")

    # 5. ImageField valid PNG
    img = await Img.objects.create(name="logo", image=PNG_BYTES)
    assert os.path.exists(os.path.join(media, img.image.name))
    print("IMAGE FIELD OK")

    # 6. ImageField invalid raises
    try:
        await Img.objects.create(name="bad", image=b"not an image")
        raise AssertionError("invalid image should have raised")
    except AssertionError:
        raise
    except Exception as e:
        print("IMAGE VALIDATION OK:", type(e).__name__)

    clear_storage()
    print("ALL CHECKS PASSED")

asyncio.run(main())
'''

    with tempfile.NamedTemporaryFile(mode="w", suffix=".py", delete=False) as f:
        f.write(script)
        script_path = f.name

    try:
        env = os.environ.copy()
        project_root = os.path.abspath(
            os.path.join(os.path.dirname(__file__), "../..")
        )
        # Prefer the local source tree over any installed wheel.
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
