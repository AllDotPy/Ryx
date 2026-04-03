"""
Unit tests for Ryx field functionality.
"""

import pytest
from datetime import datetime, date
from decimal import Decimal
import uuid

# Mock ryx_core
import sys
import types
mock_core = types.ModuleType("ryx.ryx_core")
sys.modules["ryx.ryx_core"] = mock_core

from ryx.fields import (
    Field, AutoField, BigAutoField, BigIntField, BooleanField, CharField,
    DateField, DateTimeField, DecimalField, EmailField, FloatField,
    IntField, TextField, TimeField, URLField, UUIDField,
)
from ryx.exceptions import ValidationError


class TestFieldBase:
    """Test base Field class functionality."""

    def test_field_with_options(self):
        """Test Field with explicit options."""
        field = Field(primary_key=True, null=True, blank=True, default="test")
        assert field.primary_key is True
        assert field.null is True
        assert field.blank is True
        assert field.default == "test"

    def test_field_has_default(self):
        """Test has_default() method."""
        field_without_default = Field()
        field_with_default = Field(default="test")
        
        assert not field_without_default.has_default()
        assert field_with_default.has_default()


class TestCharField:
    """Test CharField functionality."""

    def test_char_field_creation(self):
        field = CharField(max_length=100)
        assert field.max_length == 100

    def test_char_field_validation(self):
        field = CharField(max_length=5)

        # Valid
        assert field.clean("hello") == "hello"

        # Too long
        with pytest.raises(ValidationError):
            field.clean("this is too long")

    def test_char_field_to_python(self):
        field = CharField()
        assert field.to_python("string") == "string"
        assert field.to_python(None) is None

    def test_char_field_to_db(self):
        field = CharField()
        assert field.to_db("string") == "string"


class TestIntField:
    """Test IntField functionality."""

    def test_int_field_creation(self):
        field = IntField()
        assert field.min_value is None
        assert field.max_value is None

        field = IntField(min_value=0, max_value=100)
        assert field.min_value == 0
        assert field.max_value == 100

    def test_int_field_validation(self):
        field = IntField(min_value=0, max_value=10)

        # Valid
        assert field.clean(5) == 5

        # Too small
        with pytest.raises(ValidationError):
            field.clean(-1)

        # Too large
        with pytest.raises(ValidationError):
            field.clean(11)

    def test_int_field_to_python(self):
        field = IntField()
        assert field.to_python(42) == 42
        assert field.to_python("42") == 42
        assert field.to_python(None) is None

    def test_int_field_to_db(self):
        field = IntField()
        assert field.to_db(42) == 42


class TestBooleanField:
    """Test BooleanField functionality."""

    def test_boolean_field_to_python(self):
        field = BooleanField()
        assert field.to_python(True) is True
        assert field.to_python(False) is False
        assert field.to_python(1) is True
        assert field.to_python(0) is False
        assert field.to_python("true") is True
        assert field.to_python("false") is False
        assert field.to_python(None) is None

    def test_boolean_field_to_db(self):
        field = BooleanField()
        assert field.to_db(True) == 1
        assert field.to_db(False) == 0


class TestFloatField:
    """Test FloatField functionality."""

    def test_float_field_to_python(self):
        field = FloatField()
        assert field.to_python(3.14) == 3.14
        assert field.to_python("3.14") == 3.14
        assert field.to_python(None) is None

    def test_float_field_to_db(self):
        field = FloatField()
        assert field.to_db(3.14) == 3.14


class TestDecimalField:
    """Test DecimalField functionality."""

    def test_decimal_field_creation(self):
        field = DecimalField(max_digits=10, decimal_places=2)
        assert field.max_digits == 10
        assert field.decimal_places == 2

    def test_decimal_field_to_python(self):
        field = DecimalField()
        assert field.to_python(Decimal("10.50")) == Decimal("10.50")
        assert field.to_python("10.50") == Decimal("10.50")
        assert field.to_python(10.5) == Decimal("10.5")

    def test_decimal_field_to_db(self):
        field = DecimalField()
        assert field.to_db(Decimal("10.50")) == "10.50"


class TestDateTimeField:
    """Test DateTimeField functionality."""

    def test_datetime_field_to_python(self):
        field = DateTimeField()
        dt = datetime(2023, 1, 1, 12, 0, 0)
        assert field.to_python(dt) == dt
        assert field.to_python("2023-01-01T12:00:00") == dt
        assert field.to_python(None) is None

    def test_datetime_field_to_db(self):
        field = DateTimeField()
        dt = datetime(2023, 1, 1, 12, 0, 0)
        assert field.to_db(dt) == "2023-01-01T12:00:00.000000"


class TestDateField:
    """Test DateField functionality."""

    def test_date_field_to_python(self):
        field = DateField()
        d = date(2023, 1, 1)
        assert field.to_python(d) == d
        assert field.to_python("2023-01-01") == d

    def test_date_field_to_db(self):
        field = DateField()
        d = date(2023, 1, 1)
        assert field.to_db(d) == "2023-01-01"


class TestUUIDField:
    """Test UUIDField functionality."""

    def test_uuid_field_to_python(self):
        field = UUIDField()
        test_uuid = uuid.uuid4()
        assert field.to_python(test_uuid) == test_uuid
        assert field.to_python(str(test_uuid)) == test_uuid

    def test_uuid_field_to_db(self):
        field = UUIDField()
        test_uuid = uuid.uuid4()
        assert field.to_db(test_uuid) == str(test_uuid)


class TestEmailField:
    """Test EmailField functionality."""

    def test_email_field_validation(self):
        field = EmailField()

        # Valid emails
        assert field.clean("test@example.com") == "test@example.com"
        assert field.clean("user.name+tag@domain.co.uk") == "user.name+tag@domain.co.uk"

        # Invalid emails
        with pytest.raises(ValidationError):
            field.clean("invalid-email")

        with pytest.raises(ValidationError):
            field.clean("test@")

        with pytest.raises(ValidationError):
            field.clean("@example.com")


class TestURLField:
    """Test URLField functionality."""

    def test_url_field_validation(self):
        field = URLField()

        # Valid URLs
        assert field.clean("https://example.com") == "https://example.com"
        assert field.clean("http://localhost:8000/path") == "http://localhost:8000/path"

        # Invalid URLs
        with pytest.raises(ValidationError):
            field.clean("not-a-url")

        with pytest.raises(ValidationError):
            field.clean("ftp://example.com")


class TestAutoField:
    """Test AutoField functionality."""

    def test_auto_field_creation(self):
        field = AutoField()
        assert field.primary_key is True
        assert field.editable is False

    def test_big_auto_field(self):
        field = BigAutoField()
        assert field.primary_key is True
        assert field.editable is False


class TestTextField:
    """Test TextField functionality."""

    def test_text_field_creation(self):
        field = TextField()
        assert field.max_length is None

        field = TextField(max_length=1000)
        assert field.max_length == 1000

    def test_text_field_validation(self):
        field = TextField(max_length=10)

        # Valid
        assert field.clean("short") == "short"

        # Too long
        with pytest.raises(ValidationError):
            field.clean("this text is way too long for the field")


class TestFieldValidation:
    """Test field validation behavior."""

    def test_required_field_validation(self):
        """Test that null=False prevents None values."""
        field = CharField(max_length=100, null=False)
        
        # Should pass with a value
        field.validate("value")
        
        # Should fail when None but field is required
        with pytest.raises(ValidationError):
            field.validate(None)

    def test_blank_field_validation(self):
        """Test blank=True allows empty strings."""
        field = CharField(max_length=100, blank=True, null=False)
        
        # Should allow empty string when blank=True
        field.validate("")
        
        # Create a new field with blank=False
        field2 = CharField(max_length=100, blank=False, null=False)
        # Should fail on empty string when blank=False
        with pytest.raises(ValidationError):
            field2.validate("")