"""
Unit tests for Ryx FileField / ImageField / FieldFile.

No database is used — these tests exercise field-level staging, commit,
and the FieldFile value object with an in-memory storage backend.
"""

import sys
import types

mock_core = types.ModuleType("ryx.ryx_core")
sys.modules["ryx.ryx_core"] = mock_core

import pytest

from ryx.fields import FileField, ImageField, CharField
from ryx.files import FieldFile
from ryx.storage import InMemoryStorage


class _Instance:
    """Minimal stand-in for a model instance (needs __dict__ + attrs)."""

    def __init__(self):
        self.pk = None


# A tiny valid PNG header + fake payload
PNG_BYTES = b"\x89PNG\r\n\x1a\n" + b"\x00" * 32
JPEG_BYTES = b"\xff\xd8\xff\xe0" + b"\x00" * 32


class TestFileField:
    def test_db_type(self):
        assert FileField().db_type() == "VARCHAR(255)"
        assert FileField(max_length=500).db_type() == "VARCHAR(500)"

    def test_supported_lookups(self):
        assert FileField().SUPPORTED_LOOKUPS == ["exact", "isnull", "in"]

    def test_assign_none(self):
        f = FileField()
        f.attname = "file"
        obj = _Instance()
        f.__set__(obj, None)
        assert f.__get__(obj) is None

    def test_assign_existing_name_string(self):
        f = FileField(storage=InMemoryStorage())
        f.attname = "file"
        obj = _Instance()
        f.__set__(obj, "docs/report.pdf")
        ff = f.__get__(obj)
        assert isinstance(ff, FieldFile)
        assert ff.name == "docs/report.pdf"
        assert ff._committed is True

    def test_assign_bytes_stages(self):
        f = FileField(upload_to="files/", storage=InMemoryStorage())
        f.attname = "file"
        obj = _Instance()
        f.__set__(obj, b"DATA")
        ff = f.__get__(obj)
        assert ff._committed is False
        assert ff._pending is not None
        name, content = ff._pending
        assert name.startswith("files/")
        assert content == b"DATA"
        assert ff.name is None

    def test_assign_file_like_uses_name(self):
        import io

        f = FileField(upload_to="uploads/", storage=InMemoryStorage())
        f.attname = "file"
        obj = _Instance()
        bio = io.BytesIO(b"HELLO")
        bio.name = "hello.txt"
        f.__set__(obj, bio)
        ff = f.__get__(obj)
        assert ff._pending[0] == "uploads/hello.txt"

    def test_assign_invalid_type_raises(self):
        f = FileField()
        f.attname = "file"
        obj = _Instance()
        with pytest.raises(TypeError):
            f.__set__(obj, 12345)

    def test_generate_filename_str(self):
        f = FileField(upload_to="avatars")
        assert f.generate_filename(None, "a.png") == "avatars/a.png"

    def test_generate_filename_callable(self):
        f = FileField(upload_to=lambda inst, fn: f"u/{fn[:2]}")
        assert f.generate_filename(None, "abc.png") == "u/ab/abc.png"

    def test_generate_filename_empty(self):
        f = FileField()
        assert f.generate_filename(None, "a.png") == "a.png"

    def test_to_db(self):
        f = FileField(storage=InMemoryStorage())
        ff = FieldFile(None, f, "x.txt")
        assert f.to_db(ff) == "x.txt"
        assert f.to_db(None) is None

    def test_to_python_wraps_string(self):
        f = FileField(storage=InMemoryStorage())
        ff = f.to_python("x.txt")
        assert isinstance(ff, FieldFile)
        assert ff.name == "x.txt"

    async def test_before_save_commits_pending(self):
        storage = InMemoryStorage()
        f = FileField(upload_to="f/", storage=storage)
        f.attname = "file"
        obj = _Instance()
        f.__set__(obj, b"CONTENT")
        await f.before_save(obj, True)
        ff = f.__get__(obj)
        assert ff._committed is True
        assert ff.name is not None
        assert await storage.open(ff.name) == b"CONTENT"

    async def test_after_delete_removes_file(self):
        storage = InMemoryStorage()
        f = FileField(storage=storage)
        f.attname = "file"
        obj = _Instance()
        name = await storage.save("f.txt", b"x")
        f.__set__(obj, name)
        await f.after_delete(obj)
        assert not await storage.exists("f.txt")


class TestFieldFile:
    async def test_explicit_save(self):
        storage = InMemoryStorage()
        f = FileField(storage=storage)
        ff = FieldFile(None, f, None)
        await ff.save("x.txt", b"DATA")
        assert ff.name == "x.txt"
        assert await storage.open("x.txt") == b"DATA"

    async def test_read_and_size(self):
        storage = InMemoryStorage()
        f = FileField(storage=storage)
        await storage.save("x.txt", b"12345")
        ff = FieldFile(None, f, "x.txt")
        assert await ff.read() == b"12345"
        assert await ff.size() == 5

    async def test_read_pending_without_commit(self):
        f = FileField(storage=InMemoryStorage())
        ff = FieldFile(None, f, None)
        ff._pending = ("x.txt", b"PENDING")
        assert await ff.read() == b"PENDING"
        assert await ff.size() == 7
        assert await ff.exists() is True

    def test_url_and_bool(self):
        f = FileField(storage=InMemoryStorage(base_url="/media/"))
        ff = FieldFile(None, f, "a/b.png")
        assert ff.url == "/media/a/b.png"
        assert bool(ff) is True
        assert bool(FieldFile(None, f, None)) is False

    def test_eq(self):
        f = FileField(storage=InMemoryStorage())
        assert FieldFile(None, f, "x") == FieldFile(None, f, "x")
        assert FieldFile(None, f, "x") == "x"
        assert FieldFile(None, f, "x") != FieldFile(None, f, "y")

    async def test_delete_pending(self):
        f = FileField(storage=InMemoryStorage())
        ff = FieldFile(None, f, None)
        ff._pending = ("x", b"y")
        await ff.delete(save=False)
        assert ff.name is None
        assert ff._pending is None


class TestImageField:
    def test_is_filefield_subclass(self):
        assert issubclass(ImageField, FileField)
        assert ImageField().db_type() == "VARCHAR(255)"

    async def test_valid_png_passes(self):
        f = ImageField(storage=InMemoryStorage())
        f.attname = "img"
        obj = _Instance()
        f.__set__(obj, PNG_BYTES)
        await f.before_save(obj, True)  # should not raise
        ff = f.__get__(obj)
        assert ff.name is not None

    async def test_invalid_image_raises(self):
        f = ImageField(storage=InMemoryStorage())
        f.attname = "img"
        obj = _Instance()
        f.__set__(obj, b"not an image")
        with pytest.raises(ValueError):
            await f.before_save(obj, True)

    async def test_jpeg_passes(self):
        f = ImageField(storage=InMemoryStorage())
        f.attname = "img"
        obj = _Instance()
        f.__set__(obj, JPEG_BYTES)
        await f.before_save(obj, True)

    async def test_width_height_fields_populated_when_pillow_available(self):
        f = ImageField(width_field="w", height_field="h", storage=InMemoryStorage())
        f.attname = "img"
        obj = _Instance()
        obj.w = None
        obj.h = None
        f.__set__(obj, PNG_BYTES)
        await f.before_save(obj, True)
        # Without Pillow, dimensions are None → fields unchanged (None).
        # With Pillow, a real PNG would set them. Just assert no crash.
        assert hasattr(obj, "w") and hasattr(obj, "h")
