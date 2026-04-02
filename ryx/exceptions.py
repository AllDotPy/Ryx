# 
# Ryx ORM — Python Exception Hierarchy
#
# We mirror Django's exception structure so that developers familiar with
# Django feel at home. All exceptions inherit from RyxError so users can
# catch everything with a single `except RyxError`.
#
# These Python exceptions are raised by the pure-Python QuerySet / Model
# layer. The Rust layer raises generic RuntimeError / ValueError which the
# Python layer re-wraps into these typed exceptions where appropriate.
# 


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
    """Raised when a model instance fails field validation.

    Not yet implemented — reserved for a future version that adds
    per-field validators (max_length, min_value, etc.).
    """