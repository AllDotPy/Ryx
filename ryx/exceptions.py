"""
Ryx ORM — Python Exception Hierarchy

We mirror Django's exception structure so that developers familiar with
Django feel at home. All exceptions inherit from RyxError so users can
catch everything with a single `except RyxError`.

These Python exceptions are raised by the pure-Python QuerySet / Model
layer. The Rust layer raises generic RuntimeError / ValueError which the
Python layer re-wraps into these typed exceptions where appropriate.
"""

from typing import Any

####
##     BASE RYX EXCEPTION 
#####
class RyxError(Exception):
    """Base class for all Ryx ORM exceptions.

    Catch this to handle any ORM-related error::

        try:
            user = await User.objects.get(pk=42)
        except Ryx.RyxError as e:
            print(f"ORM error: {e}")
    """


####
##     RYX DATABASE ERROR 
#####
class DatabaseError(RyxError):
    """Raised when the database returns an error.

    Wraps underlying sqlx/driver errors. The original error message is
    preserved in the exception's string representation.
    """


####
##     RYX POOL NOT INITIALIZED EXCEPTION 
#####
class PoolNotInitialized(RyxError):
    """Raised when an ORM operation is attempted before ``Ryx.setup()``."""


####
##     RYX DOES NOT EXIST EXCEPTION 
#####
class DoesNotExist(RyxError):
    """Raised by ``.get()`` when no matching row is found.

    Each Model subclass also gets its own ``DoesNotExist`` attribute
    (set by the metaclass) for more specific catching::

        try:
            post = await Post.objects.get(pk=999)
        except Post.DoesNotExist:
            print("Post not found")
    """


####
##     RYX MULTIPLE OBJECTS RETURNED EXCEPTION 
#####
class MultipleObjectsReturned(RyxError):
    """Raised by ``.get()`` when more than one matching row is found.

    Use ``.filter()`` when you expect multiple results, or add more
    filter conditions to narrow down to a single row.
    """


####
##     RYX FIELD ERROR 
#####
class FieldError(RyxError):
    """Raised when an unknown field is referenced in a query.

    Example: ``Post.objects.filter(nonexistent_field=42)``
    """


####
##     RYX VALIDATION ERROR 
#####
class ValidationError(RyxError):
    """Raised when field or model validation fails.

    Attributes:
        errors: dict mapping field names (or ``"__all__"`` for non-field errors)
                to a list of error message strings.

    Example::

        raise ValidationError({"title": ["Too short", "Must start with uppercase"]})
        raise ValidationError({"__all__": ["Event dates overlap"]})

    Or for a single non-field error::

        raise ValidationError("Something went wrong")
    """

    def __init__(self, errors: Any) -> None:
        if isinstance(errors, str):
            # Convenience: a plain string is treated as a non-field error.
            self.errors: dict[str, list[str]] = {"__all__": [errors]}
        elif isinstance(errors, list):
            self.errors = {"__all__": [str(e) for e in errors]}
        elif isinstance(errors, dict):
            # Normalise values to list[str].
            self.errors = {
                field: [str(msg)] if isinstance(msg, str) else [str(m) for m in msg]
                for field, msg in errors.items()
            }
        else:
            self.errors = {"__all__": [str(errors)]}

        super().__init__(str(self.errors))

    def merge(self, other: "ValidationError") -> "ValidationError":
        """Merge another ValidationError into this one and return self."""
        for field, msgs in other.errors.items():
            self.errors.setdefault(field, []).extend(msgs)
        return self

    def __repr__(self) -> str:
        return f"ValidationError({self.errors!r})"