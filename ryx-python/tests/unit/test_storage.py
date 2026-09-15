"""
Unit tests for Ryx pluggable file storage.

These tests use only the storage module (no DB, no Rust core), but we mock
``ryx.ryx_core`` first to avoid importing the compiled extension transitively.
"""

import sys
import types

mock_core = types.ModuleType("ryx.ryx_core")
sys.modules["ryx.ryx_core"] = mock_core

import pytest

from ryx.storage import (
    FileSystemStorage,
    InMemoryStorage,
    Storage,
    clear_storage,
    configure_storage,
    get_storage,
    default_storage,
)


class TestInMemoryStorage:
    async def test_save_open_exists_delete(self):
        s = InMemoryStorage()
        name = await s.save("a/b.txt", b"hello")
        assert name == "a/b.txt"
        assert await s.exists("a/b.txt")
        assert await s.open("a/b.txt") == b"hello"
        assert await s.size("a/b.txt") == 5
        await s.delete("a/b.txt")
        assert not await s.exists("a/b.txt")
        # idempotent delete
        await s.delete("a/b.txt")

    async def test_collision_resolution(self):
        s = InMemoryStorage()
        a = await s.save("f.txt", b"1")
        b = await s.save("f.txt", b"2")
        assert a == "f.txt"
        assert a != b
        assert b.startswith("f_")
        assert b.endswith(".txt")
        assert await s.open(a) == b"1"
        assert await s.open(b) == b"2"

    async def test_url(self):
        s = InMemoryStorage(base_url="https://cdn.example.com/media")
        assert s.url("x/y.png") == "https://cdn.example.com/media/x/y.png"

    async def test_rejects_traversal(self):
        s = InMemoryStorage()
        with pytest.raises(ValueError):
            await s.save("../evil.txt", b"x")
        with pytest.raises(ValueError):
            await s.save("/abs.txt", b"x")

    async def test_open_missing_raises(self):
        s = InMemoryStorage()
        with pytest.raises(FileNotFoundError):
            await s.open("nope.txt")


class TestFileSystemStorage:
    async def test_save_open_exists_delete(self, tmp_path):
        s = FileSystemStorage(root=tmp_path, base_url="/media/")
        name = await s.save("avatars/a.png", b"PNGDATA")
        assert name == "avatars/a.png"
        assert await s.exists("avatars/a.png")
        assert await s.open("avatars/a.png") == b"PNGDATA"
        assert await s.size("avatars/a.png") == 7
        assert (tmp_path / "avatars" / "a.png").exists()
        assert s.url("avatars/a.png") == "/media/avatars/a.png"
        assert s.path("avatars/a.png") == str(tmp_path / "avatars" / "a.png")
        await s.delete("avatars/a.png")
        assert not await s.exists("avatars/a.png")

    async def test_collision_resolution(self, tmp_path):
        s = FileSystemStorage(root=tmp_path)
        a = await s.save("f.txt", b"1")
        b = await s.save("f.txt", b"2")
        assert a == "f.txt"
        assert a != b
        assert b.startswith("f_")

    async def test_rejects_traversal(self, tmp_path):
        s = FileSystemStorage(root=tmp_path)
        with pytest.raises(ValueError):
            await s.save("../evil.txt", b"x")
        with pytest.raises(ValueError):
            await s.open("../../etc/passwd")

    async def test_listdir(self, tmp_path):
        s = FileSystemStorage(root=tmp_path)
        await s.save("docs/a.txt", b"a")
        await s.save("docs/b.txt", b"b")
        entries = dict(await s.listdir("docs"))
        assert set(entries.keys()) == {"a.txt", "b.txt"}
        assert all(kind == "file" for kind in entries.values())

    async def test_delete_missing_is_noop(self, tmp_path):
        s = FileSystemStorage(root=tmp_path)
        await s.delete("nope.txt")


class TestRegistry:
    def test_default_returns_filesystem(self):
        clear_storage()
        s = get_storage()
        assert isinstance(s, FileSystemStorage)
        clear_storage()

    def test_configure_and_get(self):
        clear_storage()
        backend = InMemoryStorage()
        configure_storage(backend)
        assert get_storage() is backend
        assert default_storage() is backend
        clear_storage()

    def test_custom_storage_subclass(self):
        class MyStorage(Storage):
            async def save(self, name, content):
                return name

            async def open(self, name):
                return b""

            async def delete(self, name):
                return None

            async def exists(self, name):
                return False

            def url(self, name):
                return f"custom://{name}"

            async def size(self, name):
                return 0

        clear_storage()
        configure_storage(MyStorage())
        assert get_storage().url("x") == "custom://x"
        clear_storage()
