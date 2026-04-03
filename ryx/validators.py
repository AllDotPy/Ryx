"""
Ryx ORM — Validation System

Two levels of validation:
  1. Field-level  : each Field carries zero or more Validator instances that
     check a single value (max_length, min_value, regex, not_blank, ...).
  2. Model-level  : Model.clean() is an async hook the user overrides to add
     cross-field validation (e.g. end_date > start_date).

ValidationError carries a dict  { field_name: [error_message, ...] }  so the
caller can show per-field error messages (useful for API responses).

Usage (field level)::

  class Post(Model):
      title = CharField(max_length=200, validators=[MinLengthValidator(5)])
      age   = IntField(validators=[RangeValidator(0, 150)])

Usage (model level)::

  class Event(Model):
      start = DateTimeField()
      end   = DateTimeField()

      async def clean(self):
          if self.end <= self.start:
              raise ValidationError({"end": ["end must be after start"]})

Field declarations also accept shorthand kwargs that are automatically
converted to validators by the Field constructor:
  CharField(max_length=100)   → MaxLengthValidator(100)
  IntField(min_value=0)       → MinValueValidator(0)
  CharField(blank=False)      → NotBlankValidator()
"""

from __future__ import annotations

import re
from typing import Any, Callable, Optional

from ryx.exceptions import ValidationError 


####
##      BASE VALIDATOR PROTOCOL
##### 
class Validator:
    """Base class for all field validators.

    Subclass and override ``__call__`` to implement custom validation logic.
    Raise :exc:`ValidationError` when the value is invalid.

    Example::

        class StartsWithUppercaseValidator(Validator):
            message = "Must start with an uppercase letter."

            def __call__(self, value):
                if value and not value[0].isupper():
                    raise ValidationError(self.message)
    """

    message: str = "Invalid value."

    def __call__(self, value: Any) -> None:
        """Validate ``value``. Raise ValidationError if invalid."""
        raise NotImplementedError

    def validate(self, value: Any) -> None:
        """Alias for __call__ for compatibility."""
        return self.__call__(value)


####
##     FUNCTION VALIDATOR
#####
class FunctionValidator(Validator):
    """Wrap a plain callable as a validator.

    Usage::

        is_positive = FunctionValidator(lambda v: v > 0, "Must be positive")
    """

    def __init__(self, fn: Callable[[Any], bool], message: str) -> None:
        self._fn      = fn
        self.message  = message

    def __call__(self, value: Any) -> None:
        if value is not None and not self._fn(value):
            raise ValidationError(self.message)


####
##      NOT NULL VALIDATOR
#####
class NotNullValidator(Validator):
    """Reject None / empty values.

    Applied automatically when a field has ``null=False, blank=False``.
    """
    message = "This field may not be null."

    def __call__(self, value: Any) -> None:
        if value is None:
            raise ValidationError(self.message)


####
##      NOT BLANK VALIDATOR
#####
class NotBlankValidator(Validator):
    """Reject empty strings (strings of only whitespace count as blank).

    Applied automatically when a CharField / TextField has ``blank=False``.
    """
    message = "This field may not be blank."

    def __call__(self, value: Any) -> None:
        if isinstance(value, str) and not value.strip():
            raise ValidationError(self.message)


####
##      MAX LENGTH VALIDATOR
#####
class MaxLengthValidator(Validator):
    """Reject strings exceeding ``max_length`` characters."""

    def __init__(self, max_length: int) -> None:
        self.max_length = max_length
        self.message    = f"Ensure this value has at most {max_length} characters."

    def __call__(self, value: Any) -> None:
        if value is not None and len(str(value)) > self.max_length:
            raise ValidationError(self.message)


####
##      MIN LENGTH VALIDATOR
#####
class MinLengthValidator(Validator):
    """Reject strings shorter than ``min_length`` characters."""

    def __init__(self, min_length: int) -> None:
        self.min_length = min_length
        self.message    = f"Ensure this value has at least {min_length} characters."

    def __call__(self, value: Any) -> None:
        if value is not None and len(str(value)) < self.min_length:
            raise ValidationError(self.message)


####
##      MIN VALUE VALIDATOR
#####
class MinValueValidator(Validator):
    """Reject numeric values below ``min_value``."""

    def __init__(self, min_value) -> None:
        self.min_value = min_value
        self.message   = f"Ensure this value is greater than or equal to {min_value}."

    def __call__(self, value: Any) -> None:
        if value is not None and value < self.min_value:
            raise ValidationError(self.message)


####
##      MAX VALUE VALIDATOR
#####
class MaxValueValidator(Validator):
    """Reject numeric values above ``max_value``."""

    def __init__(self, max_value) -> None:
        self.max_value = max_value
        self.message   = f"Ensure this value is less than or equal to {max_value}."

    def __call__(self, value: Any) -> None:
        if value is not None and value > self.max_value:
            raise ValidationError(self.message)


####
##      RANGE VALIDATOR
#####
class RangeValidator(Validator):
    """Reject values outside [min_value, max_value]."""

    def __init__(self, min_value, max_value) -> None:
        self.min_value = min_value
        self.max_value = max_value
        self.message   = f"Value must be between {min_value} and {max_value}."

    def __call__(self, value: Any) -> None:
        if value is not None and not (self.min_value <= value <= self.max_value):
            raise ValidationError(self.message)


####
##      REGEX VALIDATOR
#####
class RegexValidator(Validator):
    """Reject strings that do not match the given regular expression."""

    def __init__(self, pattern: str, message: Optional[str] = None, flags: int = 0) -> None:
        self._pattern = re.compile(pattern, flags)
        self.message  = message or f"Value must match pattern: {pattern}"

    def __call__(self, value: Any) -> None:
        if value is not None and not self._pattern.search(str(value)):
            raise ValidationError(self.message)


####
##      EMAIL FORMAT VALIDATOR
#####
class EmailValidator(Validator):
    """Basic e-mail format validator."""

    _PATTERN = re.compile(r"^[^@\s]+@[^@\s]+\.[^@\s]+$")
    message   = "Enter a valid email address."

    def __call__(self, value: Any) -> None:
        if value is not None and not self._PATTERN.match(str(value)):
            raise ValidationError(self.message)


####
##      URL FORMAT VALIDATOR
#####
class URLValidator(Validator):
    """Basic URL format validator (http / https)."""

    _PATTERN = re.compile(r"^https?://[^\s/$.?#].[^\s]*$", re.IGNORECASE)
    message   = "Enter a valid URL."

    def __call__(self, value: Any) -> None:
        if value is not None and not self._PATTERN.match(str(value)):
            raise ValidationError(self.message)


####
##      CHOICES VALIDATOR
#####
class ChoicesValidator(Validator):
    """Reject values not in the allowed choices set."""

    def __init__(self, choices) -> None:
        self._choices = set(choices)
        self.message  = f"Value must be one of: {sorted(self._choices)!r}"

    def __call__(self, value: Any) -> None:
        if value is not None and value not in self._choices:
            raise ValidationError(self.message)


####
##     UNIQUE VALUE VALIDATOR (DB-ENFORCED)
#####
class UniqueValueValidator(Validator):
    """Placeholder: uniqueness is enforced at the DB level via UNIQUE constraint.

    This validator is attached automatically when ``unique=True`` is set on a
    field. It serves as documentation and is also used by the migration system
    to generate the UNIQUE constraint DDL.

    Actual uniqueness validation happens at the DB INSERT/UPDATE level and
    raises DatabaseError when violated.
    """
    message = "This value must be unique."

    def __call__(self, value: Any) -> None:
        # DB-level enforcement — no Python-side check needed.
        pass


# Validation runner
async def run_full_validation(instance) -> None:
    """Run all field validators and then model.clean() on the given instance.

    Collects ALL errors from all fields before raising a single combined
    ValidationError (instead of stopping at the first failure).

    Called automatically by Model.save() before executing SQL.
    Can also be called manually: ``await instance.full_clean()``.

    Args:
        instance: A Model instance to validate.

    Raises:
        ValidationError: If any field or the model-level clean() fails.
    """
    combined = ValidationError({})

    # Field-level validation 
    for field_name, field in instance._meta.fields.items():
        value = getattr(instance, field_name, None)

        # Run each validator registered on this field
        for validator in getattr(field, "_validators", []):
            try:
                validator(value)
            except ValidationError as e:
                combined.merge(ValidationError({field_name: list(e.errors.values())[0]}))
            except Exception as e:
                combined.merge(ValidationError({field_name: [str(e)]}))

    # Model-level validation (clean()) 
    # Call clean() only if there are no field errors yet — avoids misleading
    # cross-field errors when the inputs are individually invalid.
    if not combined.errors:
        try:
            await instance.clean()
        except ValidationError as e:
            combined.merge(e)

    # Drop any empty-error entries and raise only when concrete messages are present.
    combined.errors = {field: msgs for field, msgs in combined.errors.items() if msgs}
    if combined.errors:
        raise combined