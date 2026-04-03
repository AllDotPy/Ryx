"""
Unit tests for Ryx exception classes.
"""

import pytest

# Mock ryx_core
import sys
import types
mock_core = types.ModuleType("ryx.ryx_core")
sys.modules["ryx.ryx_core"] = mock_core

from ryx.exceptions import (
    RyxError, DatabaseError, DoesNotExist, MultipleObjectsReturned,
    FieldError, ValidationError, PoolNotInitialized
)


class TestRyxError:
    """Test base RyxError class."""

    def test_ryx_error_creation(self):
        error = RyxError("Test error")
        assert str(error) == "Test error"
        assert isinstance(error, Exception)


class TestDatabaseError:
    """Test DatabaseError class."""

    def test_database_error_creation(self):
        error = DatabaseError("Connection failed")
        assert str(error) == "Connection failed"
        assert isinstance(error, RyxError)


class TestDoesNotExist:
    """Test DoesNotExist class."""

    def test_does_not_exist_creation(self):
        error = DoesNotExist("No matching object found")
        assert str(error) == "No matching object found"
        assert isinstance(error, RyxError)


class TestMultipleObjectsReturned:
    """Test MultipleObjectsReturned class."""

    def test_multiple_objects_returned_creation(self):
        error = MultipleObjectsReturned("Multiple objects returned")
        assert str(error) == "Multiple objects returned"
        assert isinstance(error, RyxError)


class TestFieldError:
    """Test FieldError class."""

    def test_field_error_creation(self):
        error = FieldError("Unknown field referenced")
        assert str(error) == "Unknown field referenced"
        assert isinstance(error, RyxError)


class TestValidationError:
    """Test ValidationError class."""

    def test_validation_error_from_string(self):
        error = ValidationError("Simple error")
        assert error.errors == {"__all__": ["Simple error"]}
        assert str(error) == "{'__all__': ['Simple error']}"

    def test_validation_error_from_list(self):
        error = ValidationError(["error1", "error2"])
        assert error.errors == {"__all__": ["error1", "error2"]}

    def test_validation_error_from_dict(self):
        error = ValidationError({"field1": ["error1"], "field2": ["error2"]})
        assert error.errors == {"field1": ["error1"], "field2": ["error2"]}

    def test_validation_error_from_dict_with_strings(self):
        error = ValidationError({"field1": "error1", "field2": "error2"})
        assert error.errors == {"field1": ["error1"], "field2": ["error2"]}

    def test_validation_error_from_dict_with_lists(self):
        error = ValidationError({"field1": ["error1", "error2"]})
        assert error.errors == {"field1": ["error1", "error2"]}

    def test_validation_error_from_other_type(self):
        error = ValidationError(123)
        assert error.errors == {"__all__": ["123"]}

    def test_validation_error_merge(self):
        error1 = ValidationError({"field1": ["error1"]})
        error2 = ValidationError({"field1": ["error2"], "field2": ["error3"]})

        error1.merge(error2)
        assert error1.errors == {
            "field1": ["error1", "error2"],
            "field2": ["error3"]
        }

    def test_validation_error_repr(self):
        error = ValidationError({"field": ["error"]})
        assert repr(error) == "ValidationError({'field': ['error']})"


class TestPoolNotInitialized:
    """Test PoolNotInitialized class."""

    def test_pool_not_initialized_creation(self):
        error = PoolNotInitialized("Database pool not initialized")
        assert str(error) == "Database pool not initialized"
        assert isinstance(error, RyxError)


class TestExceptionHierarchy:
    """Test that all exceptions inherit properly from RyxError."""

    def test_all_exceptions_inherit_from_ryx_error(self):
        exceptions = [
            DatabaseError,
            DoesNotExist,
            MultipleObjectsReturned,
            FieldError,
            ValidationError,
            PoolNotInitialized,
        ]

        for exc_class in exceptions:
            error = exc_class("test")
            assert isinstance(error, RyxError)
            assert isinstance(error, Exception)