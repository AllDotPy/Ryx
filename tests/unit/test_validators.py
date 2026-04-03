"""
Unit tests for Ryx validator functionality.
"""

import pytest

# Mock ryx_core
import sys
import types
mock_core = types.ModuleType("ryx.ryx_core")
sys.modules["ryx.ryx_core"] = mock_core

from ryx.validators import (
    Validator, MaxLengthValidator, MinLengthValidator, MaxValueValidator,
    MinValueValidator, RangeValidator, RegexValidator, EmailValidator,
    URLValidator, NotBlankValidator, NotNullValidator, ChoicesValidator,
    ValidationError, run_full_validation,
)
from ryx.fields import CharField, IntField


class TestBaseValidator:
    """Test base Validator class."""

    def test_validator_creation(self):
        validator = Validator()
        assert hasattr(validator, 'validate')


class TestMaxLengthValidator:
    """Test MaxLengthValidator."""

    def test_valid_length(self):
        validator = MaxLengthValidator(10)
        validator.validate("short")  # Should not raise

    def test_too_long(self):
        validator = MaxLengthValidator(5)
        with pytest.raises(ValidationError, match="at most 5 characters"):
            validator.validate("this is too long")


class TestMinLengthValidator:
    """Test MinLengthValidator."""

    def test_valid_length(self):
        validator = MinLengthValidator(3)
        validator.validate("long enough")  # Should not raise

    def test_too_short(self):
        validator = MinLengthValidator(10)
        with pytest.raises(ValidationError, match="at least 10 characters"):
            validator.validate("short")


class TestMaxValueValidator:
    """Test MaxValueValidator."""

    def test_valid_value(self):
        validator = MaxValueValidator(100)
        validator.validate(50)  # Should not raise

    def test_too_large(self):
        validator = MaxValueValidator(10)
        with pytest.raises(ValidationError, match="less than or equal to 10"):
            validator.validate(15)


class TestMinValueValidator:
    """Test MinValueValidator."""

    def test_valid_value(self):
        validator = MinValueValidator(10)
        validator.validate(50)  # Should not raise

    def test_too_small(self):
        validator = MinValueValidator(100)
        with pytest.raises(ValidationError, match="greater than or equal to 100"):
            validator.validate(50)


class TestRangeValidator:
    """Test RangeValidator."""

    def test_valid_range(self):
        validator = RangeValidator(10, 100)
        validator.validate(50)  # Should not raise

    def test_too_small(self):
        validator = RangeValidator(10, 100)
        with pytest.raises(ValidationError):
            validator.validate(5)

    def test_too_large(self):
        validator = RangeValidator(10, 100)
        with pytest.raises(ValidationError):
            validator.validate(150)


class TestRegexValidator:
    """Test RegexValidator."""

    def test_valid_regex(self):
        validator = RegexValidator(r'^\d{3}-\d{2}-\d{4}$')
        validator.validate("123-45-6789")  # Should not raise

    def test_invalid_regex(self):
        validator = RegexValidator(r'^\d{3}-\d{2}-\d{4}$')
        with pytest.raises(ValidationError):
            validator.validate("invalid-ssn")


class TestEmailValidator:
    """Test EmailValidator."""

    def test_valid_emails(self):
        validator = EmailValidator()
        validator.validate("test@example.com")
        validator.validate("user.name+tag@domain.co.uk")

    def test_invalid_emails(self):
        validator = EmailValidator()
        with pytest.raises(ValidationError):
            validator.validate("invalid-email")

        with pytest.raises(ValidationError):
            validator.validate("test@")

        with pytest.raises(ValidationError):
            validator.validate("@example.com")


class TestURLValidator:
    """Test URLValidator."""

    def test_valid_urls(self):
        validator = URLValidator()
        validator.validate("https://example.com")
        validator.validate("http://localhost:8000/path")

    def test_invalid_urls(self):
        validator = URLValidator()
        with pytest.raises(ValidationError):
            validator.validate("not-a-url")

        with pytest.raises(ValidationError):
            validator.validate("ftp://example.com")


class TestNotBlankValidator:
    """Test NotBlankValidator."""

    def test_valid_not_blank(self):
        validator = NotBlankValidator()
        validator.validate("has content")  # Should not raise

    def test_blank_string(self):
        validator = NotBlankValidator()
        with pytest.raises(ValidationError):
            validator.validate("")

        with pytest.raises(ValidationError):
            validator.validate("   ")


class TestNotNullValidator:
    """Test NotNullValidator."""

    def test_valid_not_null(self):
        validator = NotNullValidator()
        validator.validate("value")  # Should not raise
        validator.validate(0)  # Should not raise

    def test_null_value(self):
        validator = NotNullValidator()
        with pytest.raises(ValidationError):
            validator.validate(None)


class TestChoicesValidator:
    """Test ChoicesValidator."""

    def test_valid_choice(self):
        validator = ChoicesValidator(["red", "green", "blue"])
        validator.validate("red")  # Should not raise

    def test_invalid_choice(self):
        validator = ChoicesValidator(["red", "green", "blue"])
        with pytest.raises(ValidationError):
            validator.validate("yellow")


class TestValidationError:
    """Test ValidationError functionality."""

    def test_validation_error_creation(self):
        error = ValidationError("Simple error")
        assert error.errors == {"__all__": ["Simple error"]}

    def test_validation_error_with_dict(self):
        error = ValidationError({"field1": ["error1"], "field2": ["error2"]})
        assert error.errors == {"field1": ["error1"], "field2": ["error2"]}

    def test_validation_error_with_list(self):
        error = ValidationError(["error1", "error2"])
        assert error.errors == {"__all__": ["error1", "error2"]}

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


class TestRunFullValidation:
    """Test run_full_validation function."""

    @pytest.mark.asyncio
    async def test_run_full_validation_success(self):
        # Mock model with fields
        class MockModel:
            def __init__(self):
                self.field1 = "value1"
                self.field2 = 42

            async def clean(self):
                pass

        # Mock fields
        field1 = CharField(max_length=100)
        field1.attname = "field1"
        field2 = IntField(min_value=0)
        field2.attname = "field2"

        model = MockModel()
        model._meta = type('Meta', (), {
            'fields': {'field1': field1, 'field2': field2}
        })()

        # Should not raise
        await run_full_validation(model)

    @pytest.mark.asyncio
    async def test_run_full_validation_field_error(self):
        class MockModel:
            def __init__(self):
                self.field1 = "this is way too long for the field"

            async def clean(self):
                pass

        field1 = CharField(max_length=10)
        field1.attname = "field1"

        model = MockModel()
        model._meta = type('Meta', (), {
            'fields': {'field1': field1}
        })()

        with pytest.raises(ValidationError):
            await run_full_validation(model)

    @pytest.mark.asyncio
    async def test_run_full_validation_model_clean_error(self):
        class MockModel:
            def __init__(self):
                self.field1 = "value"

            async def clean(self):
                raise ValidationError("Model validation failed")

        field1 = CharField(max_length=100)
        field1.attname = "field1"

        model = MockModel()
        model._meta = type('Meta', (), {
            'fields': {'field1': field1}
        })()

        with pytest.raises(ValidationError):
            await run_full_validation(model)