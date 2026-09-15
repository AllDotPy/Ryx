"""
Ryx ORM — Field Classes
"""

from __future__ import annotations

import os
import uuid
import json
from datetime import date, datetime, timedelta
from decimal import Decimal
from typing import TYPE_CHECKING, Any, List, Optional, Sequence, Type

from ryx.files import FieldFile

from ryx.validators import (
    ChoicesValidator,
    EmailValidator,
    MaxLengthValidator,
    MaxValueValidator,
    MinLengthValidator,
    MinValueValidator,
    NotBlankValidator,
    NotNullValidator,
    RegexValidator,
    URLValidator,
    UniqueValueValidator,
    ValidationError,
    Validator,
)

if TYPE_CHECKING:
    from ryx.models import Model

# Deferred reverse FK descriptor registry
# Forward-reference FK targets (string names) can't install ReverseFKDescriptors
# immediately at class-definition time because the target class may not exist yet.
# We accumulate (target_ref, rel_name, source_model, fk_attname) tuples here
# and call resolve_pending_reverse_fks() after all models are defined.
_pending_reverse_fk: list = []


####    RESOLVE PENDING REVERSE FKS
def resolve_pending_reverse_fks() -> None:
    """Install all deferred ReverseFKDescriptors.

    Call this once after all Model subclasses have been defined, e.g. at the
    end of your models module or in your application startup code::

        from Ryx.fields import resolve_pending_reverse_fks
        resolve_pending_reverse_fks()

    Ryx's ModelMetaclass calls this automatically after each class definition,
    so for simple same-file definitions it resolves immediately.
    """
    from ryx.descriptors import ReverseFKDescriptor
    from ryx.relations import _resolve_model
    import sys

    still_pending = []
    for target_ref, rel_name, source_model, fk_attname in _pending_reverse_fk:
        try:
            target_model = _resolve_model(target_ref, source_model)
            if not hasattr(target_model, rel_name):
                desc = ReverseFKDescriptor(source_model, fk_attname)
                desc.__set_name__(target_model, rel_name)
                setattr(target_model, rel_name, desc)
        except (ValueError, TypeError):
            # Target not yet defined — keep for retry
            still_pending.append((target_ref, rel_name, source_model, fk_attname))

    _pending_reverse_fk.clear()
    _pending_reverse_fk.extend(still_pending)


_MISSING = object()


#####
###     BASE FIELD CLASS
#####
class Field:
    """Base class for all Ryx field types.

    Every field is a descriptor (implements ``__get__`` / ``__set__``) so
    that model instances expose field values as plain attribute access.

    Common attributes
    -----------------
    null          : bool   — Allow NULL in the database. Default: False.
    blank         : bool   — Allow empty values in validation. Default: False.
    default       : Any    — Default value or callable.
    primary_key   : bool   — Mark as primary key.
    unique        : bool   — Add UNIQUE constraint to the column.
    db_index      : bool   — Create a database index.
    choices       : list   — Restrict to these values. Adds ChoicesValidator.
    validators    : list   — Additional Validator instances.
    editable      : bool   — If False, exclude from save(). Default: True.
    help_text     : str    — Human-readable description (for docs / forms).
    verbose_name  : str    — Human-readable column name.
    db_column     : str    — Override the SQL column name.
    unique_for_date:str    — Field name — enforce uniqueness per date value.
    unique_for_month:str   — Field name — enforce uniqueness per month value.
    unique_for_year : str  — Field name — enforce uniqueness per year value.

    SUPPORTED_LOOKUPS: list[str] — Lookups allowed on this field.
    SUPPORTED_TRANSFORMS: list[str] — Transforms allowed on this field.
    """

    SUPPORTED_LOOKUPS: list[str] = []
    SUPPORTED_TRANSFORMS: list[str] = []

    attname: str = ""
    column: str = ""
    model: Optional[Type["Model"]] = None

    def __init__(
        self,
        *,
        null: bool = False,
        blank: bool = False,
        default: Any = _MISSING,
        primary_key: bool = False,
        unique: bool = False,
        db_index: bool = False,
        choices: Optional[Sequence] = None,
        validators: Optional[List[Validator]] = None,
        editable: bool = True,
        help_text: str = "",
        verbose_name: str = "",
        db_column: Optional[str] = None,
        unique_for_date: Optional[str] = None,
        unique_for_month: Optional[str] = None,
        unique_for_year: Optional[str] = None,
    ) -> None:
        self.null = null
        self.blank = blank
        self.default = default
        self.primary_key = primary_key
        self.unique = unique
        self.db_index = db_index
        self.choices = list(choices) if choices else None
        self.editable = editable
        self.help_text = help_text
        self.verbose_name = verbose_name
        self.unique_for_date = unique_for_date
        self.unique_for_month = unique_for_month
        self.unique_for_year = unique_for_year
        self._db_column = db_column

        # Build the internal validator list from explicit + shorthand options.
        self._validators: List[Validator] = list(validators or [])
        self._build_implicit_validators()

    # Implicit validator construction
    def _build_implicit_validators(self) -> None:
        """Add validators implied by field kwargs.

        Subclasses call super() then append their own.
        """
        # Not null
        if not self.null and not self.primary_key:
            self._validators.insert(0, NotNullValidator())

        # Choices
        if self.choices:
            # Extract just the values from (value, label) pairs if necessary
            vals = [c[0] if isinstance(c, (list, tuple)) else c for c in self.choices]
            self._validators.append(ChoicesValidator(vals))

        # Unique
        if self.unique:
            self._validators.append(UniqueValueValidator())

    # Descriptor protocol
    def __set_name__(self, owner: type, name: str) -> None:
        self.attname = name
        self.column = self._db_column or name

    def __get__(self, obj: Optional["Model"], objtype: Optional[type] = None) -> Any:
        if obj is None:
            return self
        value = obj.__dict__.get(self.attname, None)
        # Primary keys must not expose a default before they are set — a
        # callable default (e.g. UUID auto_create) would otherwise make
        # ``self.pk`` non-None on unsaved instances and break the INSERT path.
        if value is None and not self.primary_key:
            return self.get_default()
        return value

    def __set__(self, obj: "Model", value: Any) -> None:
        obj.__dict__[self.attname] = self.to_python(value)

    # Field API
    def contribute_to_class(self, model: Type["Model"], name: str) -> None:
        self.attname = name
        self.column = self._db_column or name
        self.model = model

    def db_type(self) -> str:
        raise NotImplementedError(f"{type(self).__name__}.db_type() not implemented")

    def to_python(self, value: Any) -> Any:
        return value

    def to_db(self, value: Any) -> Any:
        return value

    def get_default(self) -> Any:
        if self.default is _MISSING:
            return None
        return self.default() if callable(self.default) else self.default

    def has_default(self) -> bool:
        return self.default is not _MISSING

    def _validate_lookup(self, lookup: str) -> None:
        """Verify that the lookup is supported by this field type."""
        if lookup not in self.SUPPORTED_LOOKUPS:
            raise ValueError(
                f"Lookup '{lookup}' is not supported on {type(self).__name__}. "
                f"Supported lookups: {', '.join(self.SUPPORTED_LOOKUPS)}"
            )

    def _validate_transform(self, transform: str) -> None:
        """Verify that the transform is supported by this field type."""
        if transform not in self.SUPPORTED_TRANSFORMS:
            raise ValueError(
                f"Transform '{transform}' is not supported on {type(self).__name__}. "
                f"Supported transforms: {', '.join(self.SUPPORTED_TRANSFORMS)}"
            )

    def validate(self, value: Any) -> None:
        """Run all validators on ``value``.

        Raises:
            ValidationError: if any validator fails.
        """
        errors: list[str] = []
        for v in self._validators:
            try:
                v(value)
            except ValidationError as e:
                errors.extend(e.errors.get("__all__", [str(e)]))
        if errors:
            raise ValidationError(errors)

    def clean(self, value: Any) -> Any:
        """Validate and return the cleaned value.

        This is a convenience method that validates the value and returns it
        if validation passes.
        """
        self.validate(value)
        return value

    # Lifecycle hooks (no-ops by default — subclasses like FileField override)
    async def before_save(self, instance: "Model", created: bool) -> None:
        """Field-level hook invoked before the model is written.

        No-op by default. ``FileField`` uses this to commit staged files.
        """
        return None

    async def after_delete(self, instance: "Model") -> None:
        """Field-level hook invoked after the model row is deleted.

        No-op by default. ``FileField`` uses this to delete stored files.
        """
        return None

    def deconstruct(self) -> dict:
        """Return a dict representation for migration serialization."""

        return {
            "type": type(self).__name__,
            "db_type": self.db_type(),
            "null": self.null,
            "blank": self.blank,
            "unique": self.unique,
            "primary_key": self.primary_key,
            "db_index": self.db_index,
        }

    def __repr__(self) -> str:
        model_name = self.model.__name__ if self.model else "?"
        return f"<{type(self).__name__}: {model_name}.{self.attname}>"


####
###     AUTO FIELD
#####
class AutoField(Field):
    """Auto-incrementing integer primary key. Added implicitly when no PK declared."""

    SUPPORTED_LOOKUPS = ["exact", "gt", "gte", "lt", "lte", "in", "range", "isnull"]

    def __init__(self, **kw):
        kw.setdefault("primary_key", True)
        kw.setdefault("editable", False)
        super().__init__(**kw)

    def db_type(self) -> str:
        return "INTEGER"

    def to_python(self, v):
        if isinstance(v, list):
            return v[0] if v else None
        return None if v is None else int(v)

    def _build_implicit_validators(self):
        pass  # PK never needs NotNullValidator


####
###     BIG AUTO FIELD
#####
class BigAutoField(AutoField):
    """64-bit auto-increment PK."""

    def db_type(self) -> str:
        return "BIGINT"


####
###     SMALL AUTO FIELD
#####
class SmallAutoField(AutoField):
    """16-bit auto-increment PK."""

    def db_type(self) -> str:
        return "SMALLINT"


####
###     INTEGER FIELD
#####
class IntField(Field):
    """32-bit integer.

    Extra kwargs: ``min_value``, ``max_value``.
    """

    SUPPORTED_LOOKUPS = ["exact", "gt", "gte", "lt", "lte", "in", "range", "isnull"]
    SUPPORTED_TRANSFORMS = []

    def __init__(
        self, 
        *, 
        min_value = None, 
        max_value = None, 
        **kw
    ):
        super().__init__(**kw)
        if min_value is not None: 
            self._validators.append(MinValueValidator(min_value))

        if max_value is not None: 
            self._validators.append(MaxValueValidator(max_value))

        self.min_value = min_value
        self.max_value = max_value

    def db_type(self) -> str:
        return "INTEGER"
    
    def to_python(self, v): 
        return None if v is None else int(v)
    

####
###     SMALL INTEGER FIELD
#####
class SmallIntField(IntField):
    """16-bit integer (SMALLINT)."""

    SUPPORTED_LOOKUPS = ["exact", "gt", "gte", "lt", "lte", "in", "range", "isnull"]
    SUPPORTED_TRANSFORMS = []

    def db_type(self) -> str: 
        return "SMALLINT"


####
###     BIG INTEGER FIELD
#####
class BigIntField(IntField):
    """64-bit integer (BIGINT)."""

    SUPPORTED_LOOKUPS = ["exact", "gt", "gte", "lt", "lte", "in", "range", "isnull"]
    SUPPORTED_TRANSFORMS = []

    def db_type(self) -> str: 
        return "BIGINT"


####
###     POSITIVE INTEGER FIELD
#####
class PositiveIntField(IntField):
    """Integer that must be >= 0."""

    SUPPORTED_LOOKUPS = ["exact", "gt", "gte", "lt", "lte", "in", "range", "isnull"]
    SUPPORTED_TRANSFORMS = []

    def __init__(self, **kw):
        kw.setdefault("min_value", 0)
        super().__init__(**kw)

    def db_type(self) -> str: return "INTEGER"


####
###     FLOAT FIELD
#####
class FloatField(Field):
    """Double-precision float. Extra kwargs: ``min_value``, ``max_value``."""

    SUPPORTED_LOOKUPS = ["exact", "gt", "gte", "lt", "lte", "in", "range", "isnull"]
    SUPPORTED_TRANSFORMS = []

    def __init__(self, *, min_value=None, max_value=None, **kw):
        super().__init__(**kw)

        if min_value is not None: 
            self._validators.append(MinValueValidator(min_value))

        if max_value is not None: 
            self._validators.append(MaxValueValidator(max_value))

    def db_type(self) -> str: 
        return "DOUBLE PRECISION"
    
    def to_python(self, v): 
        return None if v is None else float(v)


####
###     DECIMAL FIELD
#####
class DecimalField(Field):
    """Fixed-precision decimal (NUMERIC). Extra kwargs: ``min_value``, ``max_value``."""

    SUPPORTED_LOOKUPS = ["exact", "gt", "gte", "lt", "lte", "in", "range", "isnull"]
    SUPPORTED_TRANSFORMS = []

    def __init__(
        self, 
        *, 
        max_digits: int = 10, 
        decimal_places: int = 2,
        min_value = None, 
        max_value = None, 
        **kw
    ):
        super().__init__(**kw)
        self.max_digits = max_digits
        self.decimal_places = decimal_places

        if min_value is not None: 
            self._validators.append(MinValueValidator(min_value))

        if max_value is not None: 
            self._validators.append(MaxValueValidator(max_value))

    def db_type(self) -> str: 
        return f"NUMERIC({self.max_digits}, {self.decimal_places})"
    
    def to_python(self, v): 
        return None if v is None else Decimal(str(v))
    
    def to_db(self, v):     
        return None if v is None else str(v)


####
###     BOOLEAN FIELD
#####
class BooleanField(Field):
    """Boolean (BOOLEAN)."""

    SUPPORTED_LOOKUPS = ["exact", "isnull"]
    SUPPORTED_TRANSFORMS = []

    def db_type(self) -> str:
        return "BOOLEAN"

    def to_python(self, v):
        if v is None:
            return None
        if isinstance(v, str):
            v_lower = v.lower()
            if v_lower in ("true", "1", "yes", "on"):
                return True
            elif v_lower in ("false", "0", "no", "off", ""):
                return False
        return bool(v)


####
###     NULL BOOLEAN FIELD
#####
class NullBooleanField(BooleanField):
    """Nullable boolean. Equivalent to BooleanField(null=True)."""

    def __init__(self, **kw):
        kw.setdefault("null", True)
        super().__init__(**kw)


####
###     CHAR FIELD
#####
class CharField(Field):
    """VARCHAR(max_length).

    Extra kwargs:
        max_length  : int  — Required. Maximum characters.
        min_length  : int  — Optional. Minimum characters.
        blank       : bool — Allow empty strings (default: False).
        strip       : bool — Strip leading/trailing whitespace (default: True).
    """

    SUPPORTED_LOOKUPS = [
        "exact",
        "contains",
        "icontains",
        "startswith",
        "istartswith",
        "endswith",
        "iendswith",
        "in",
        "range",
        "isnull",
    ]
    SUPPORTED_TRANSFORMS = []

    def __init__(
        self,
        *,
        max_length: int = 255,
        min_length: Optional[int] = None,
        strip: bool = True,
        **kw,
    ):
        self._strip = strip
        self.max_length = max_length
        self.min_length = min_length
        super().__init__(**kw)

        # Max length validator
        self._validators.append(MaxLengthValidator(max_length))
        if min_length is not None:
            self._validators.append(MinLengthValidator(min_length))

        if not self.blank and not self.null:
            self._validators.append(NotBlankValidator())

    def db_type(self) -> str:
        return f"VARCHAR({self.max_length})"

    def to_python(self, v):
        if v is None:
            return None
        s = str(v)
        return s.strip() if self._strip else s


####
###     SLUG FIELD
#####
class SlugField(CharField):
    """CharField that validates slug format (letters, digits, hyphens, underscores)."""

    _SLUG_RE = RegexValidator(
        r"^[-\w]+$", "Enter a valid slug (letters, digits, hyphens, underscores)."
    )

    def __init__(self, **kw):
        kw.setdefault("max_length", 50)
        super().__init__(**kw)
        self._validators.append(self._SLUG_RE)


####
###     EMAIL FIELD
#####
class EmailField(CharField):
    """CharField with e-mail format validation."""

    def __init__(self, **kw):
        kw.setdefault("max_length", 254)
        super().__init__(**kw)
        self._validators.append(EmailValidator())


####
###     URL FIELD
#####
class URLField(CharField):
    """CharField with URL format validation."""

    def __init__(self, **kw):
        kw.setdefault("max_length", 200)
        super().__init__(**kw)
        self._validators.append(URLValidator())


####
###     IP ADDRESS FIELD
#####
class IPAddressField(CharField):
    """CharField for IPv4 addresses."""

    _IP_RE = RegexValidator(r"^(\d{1,3}\.){3}\d{1,3}$", "Enter a valid IPv4 address.")

    def __init__(self, **kw):
        kw.setdefault("max_length", 15)
        super().__init__(**kw)
        self._validators.append(self._IP_RE)


####
###     TEXT FIELD
#####
class TextField(Field):
    """Unbounded text (TEXT). Extra kwargs: ``min_length``, ``max_length``."""

    def __init__(
        self,
        *,
        min_length: Optional[int] = None,
        max_length: Optional[int] = None,
        **kw,
    ):
        super().__init__(**kw)

        self.max_length = max_length

        if min_length is not None:
            self._validators.append(MinLengthValidator(min_length))

        if max_length is not None:
            self._validators.append(MaxLengthValidator(max_length))

        if not self.blank and not self.null:
            self._validators.append(NotBlankValidator())

    def db_type(self) -> str:
        return "TEXT"

    def to_python(self, v):
        return None if v is None else str(v)


####
###     BINARY FIELD
#####
class BinaryField(Field):
    """Binary blob field (BYTEA / BLOB)."""

    def db_type(self) -> str:
        return "BYTEA"

    def to_python(self, v):
        return v

    def _build_implicit_validators(self):
        pass  # binary content — skip NotBlankValidator


####
###     DATE FIELD
#####
class DateField(Field):
    """Date only (DATE). Extra kwargs: ``auto_now``, ``auto_now_add``."""

    SUPPORTED_LOOKUPS = ["exact", "gt", "gte", "lt", "lte", "in", "range", "isnull"]
    SUPPORTED_TRANSFORMS = [
        "date",
        "year",
        "month",
        "day",
        "week",
        "dow",
        "quarter",
        "iso_week",
        "iso_dow",
    ]

    def __init__(self, *, auto_now: bool = False, auto_now_add: bool = False, **kw):
        self.auto_now = auto_now
        self.auto_now_add = auto_now_add

        if auto_now or auto_now_add:
            kw.setdefault("editable", False)
        super().__init__(**kw)

    def db_type(self) -> str:
        return "DATE"

    def to_python(self, v):
        if v is None:
            return None
        if isinstance(v, datetime):
            return v.date()
        if isinstance(v, date):
            return v
        return date.fromisoformat(str(v))

    def to_db(self, v):
        return (
            None
            if v is None
            else (v.isoformat() if isinstance(v, (date, datetime)) else str(v))
        )


####
###     DATE TIME FIELD
#####
class DateTimeField(Field):
    """Timestamp (TIMESTAMP). Extra kwargs: ``auto_now``, ``auto_now_add``."""

    SUPPORTED_LOOKUPS = ["exact", "gt", "gte", "lt", "lte", "in", "range", "isnull"]
    SUPPORTED_TRANSFORMS = [
        "date",
        "year",
        "month",
        "day",
        "hour",
        "minute",
        "second",
        "week",
        "dow",
        "quarter",
        "time",
        "iso_week",
        "iso_dow",
    ]

    def __init__(
        self,
        *,
        auto_now: bool = False,
        auto_now_add: bool = False,
        **kw,
    ):
        self.auto_now = auto_now
        self.auto_now_add = auto_now_add

        if auto_now or auto_now_add:
            kw.setdefault("editable", False)
        super().__init__(**kw)

    def db_type(self) -> str:
        # Stored as TEXT so the sqlx "any" driver (no native timestamp support)
        # round-trips the ISO string losslessly across PostgreSQL/SQLite/MySQL.
        return "VARCHAR(32)"

    def to_python(self, v):
        if v is None or v == "":
            return None
        if isinstance(v, datetime):
            return v
        return datetime.fromisoformat(str(v))

    def to_db(self, v):
        if v is None:
            return None
        if isinstance(v, datetime):
            return v.strftime("%Y-%m-%dT%H:%M:%S.%f")
        return str(v)


####
###     TIME FIELD
#####
class TimeField(Field):
    """Time only (TIME)."""

    def db_type(self) -> str:
        return "TIME"

    def to_python(self, v):
        from datetime import time

        if v is None:
            return None
        if isinstance(v, time):
            return v
        return time.fromisoformat(str(v))


####
###     DURATION FIELD
#####
class DurationField(Field):
    """Python timedelta stored as BIGINT (microseconds)."""

    def db_type(self) -> str:
        return "BIGINT"

    def to_python(self, v):
        if v is None:
            return None
        if isinstance(v, timedelta):
            return v
        return timedelta(microseconds=int(v))

    def to_db(self, v):
        if v is None:
            return None
        return int(v.total_seconds() * 1_000_000)


####
###     UUID FIELD
#####
class UUIDField(Field):
    """UUID field. Stored as UUID (Postgres) or TEXT (others).

    Extra kwargs: ``auto_create`` — generate uuid4 by default.
    """

    SUPPORTED_LOOKUPS = ["exact", "in", "isnull"]
    SUPPORTED_TRANSFORMS = []

    def __init__(self, *, auto_create: bool = False, **kw):
        self.auto_create = auto_create
        if auto_create:
            kw.setdefault("default", uuid.uuid4)
        super().__init__(**kw)

    def db_type(self) -> str:
        return "UUID"

    def to_python(self, v):
        if v is None:
            return None
        return v if isinstance(v, uuid.UUID) else uuid.UUID(str(v))

    def to_db(self, v):
        return None if v is None else str(v)


####
###     JSON FIELD
#####
class JSONField(Field):
    """JSON field. Stored as JSONB (Postgres) or TEXT (others)."""

    SUPPORTED_LOOKUPS = [
        "exact",
        "gt",
        "gte",
        "lt",
        "lte",
        "in",
        "range",
        "isnull",
        "has_key",
        "has_any",
        "has_all",
        "contains",
        "contained_by",
    ]
    SUPPORTED_TRANSFORMS = ["key", "key_text", "json"]

    def db_type(self) -> str:
        return "JSONB"

    def to_python(self, v):
        if v is None:
            return None
        return json.loads(v) if isinstance(v, str) else v

    def to_db(self, v):
        return None if v is None else json.dumps(v)

    def _build_implicit_validators(self):
        pass


####
###     ARRAY FIELD
#####
class ArrayField(Field):
    """PostgreSQL ARRAY field.

    Args:
        base_field: The element type field (e.g. ``IntField()``).
    """

    def __init__(self, base_field: Field, **kw):
        self.base_field = base_field
        super().__init__(**kw)

    def db_type(self) -> str:
        return f"{self.base_field.db_type()}[]"

    def to_python(self, v):
        if v is None:
            return None
        if isinstance(v, list):
            return v
        return json.loads(v)

    def to_db(self, v):
        return None if v is None else json.dumps(v)

    def _build_implicit_validators(self):
        pass


####
###     VECTOR FIELD  (pgvector — PostgreSQL only)
#####
class VectorField(Field):
    """pgvector embedding vector (PostgreSQL only).

    Stores an N-dimensional float vector, backed by the ``vector(n)``
    PostgreSQL type (requires the ``pgvector`` extension).

    Args:
        dimensions: Vector dimensionality (e.g. 768 for OpenAI embeddings).
                    Default: 768.

    Supported lookups: ``isnull``.

    Nearest-neighbor search is performed with :meth:`QuerySet.nearest_neighbors`
    or :meth:`QuerySet.order_by_distance`.
    """

    SUPPORTED_LOOKUPS = ["isnull"]

    def __init__(self, dimensions: int = 768, **kw):
        self.dimensions = dimensions
        super().__init__(**kw)

    def db_type(self) -> str:
        return f"vector({self.dimensions})"

    def to_python(self, v):
        if v is None:
            return None
        if isinstance(v, (list, tuple)):
            return list(v)
        if isinstance(v, str):
            return json.loads(v)
        return list(v)

    def to_db(self, v):
        return None if v is None else json.dumps(list(v))

    def _build_implicit_validators(self):
        pass


####
###     FILE FIELD
#####
class FileField(Field):
    """A file stored via a pluggable :class:`~ryx.storage.Storage` backend.

    The database stores only the file's name/path (``VARCHAR``); the bytes
    live in the storage backend (local filesystem by default).

    Args:
        upload_to: Directory prefix (``str``) or callable
                   ``(instance, filename) -> str`` applied to the stored name.
        storage:   A custom storage backend. Defaults to the global storage
                   configured with :func:`ryx.configure_storage`.
        max_length: Column length for the stored name. Default: 255.

    Assignment accepts:
        - ``None`` / ``""``            — clear the file
        - ``str`` / ``os.PathLike``    — an already-stored name
        - ``bytes``                    — raw content (staged, committed on save)
        - a file-like object (``.read()``) — content (staged)
        - a :class:`~ryx.files.FieldFile`

    Example::

        class User(Model):
            avatar = FileField(upload_to="avatars/")

        user = User(avatar=b"...")
        await user.save()          # bytes written to storage
        print(user.avatar.url)
    """

    SUPPORTED_LOOKUPS = ["exact", "isnull", "in"]

    def __init__(
        self,
        *,
        upload_to: Any = "",
        storage: Any = None,
        max_length: int = 255,
        **kw,
    ) -> None:
        self.upload_to = upload_to
        self.storage = storage
        self.max_length = max_length
        super().__init__(**kw)

    def db_type(self) -> str:
        return f"VARCHAR({self.max_length})"

    # Descriptor: wrap the stored name in a FieldFile bound to the instance
    def __get__(self, obj, objtype=None):
        if obj is None:
            return self
        value = obj.__dict__.get(self.attname, None)
        if value is None:
            return None
        if isinstance(value, FieldFile):
            return value
        ff = FieldFile(obj, self, value)
        obj.__dict__[self.attname] = ff
        return ff

    def __set__(self, obj, value):
        if value is None or value == "":
            obj.__dict__[self.attname] = None
            return
        if isinstance(value, FieldFile):
            value.instance = obj
            value.field = self
            obj.__dict__[self.attname] = value
            return
        if isinstance(value, (str, os.PathLike)):
            obj.__dict__[self.attname] = FieldFile(obj, self, str(value))
            return

        # Raw content: bytes or a file-like object → stage for commit on save.
        content: Optional[bytes]
        name: Optional[str] = None
        if hasattr(value, "read"):
            name = getattr(value, "name", None)
            content = value.read()
        elif isinstance(value, (bytes, bytearray)):
            content = bytes(value)
        else:
            raise TypeError(
                f"FileField '{self.attname}' expected None, str, bytes, or a "
                f"file-like object, got {type(value).__name__}"
            )

        filename = os.path.basename(name) if name else f"file_{uuid.uuid4().hex[:7]}"
        generated = self.generate_filename(obj, filename)
        ff = FieldFile(obj, self, name=None)
        ff._committed = False
        ff._pending = (generated, content or b"")
        obj.__dict__[self.attname] = ff

    def generate_filename(self, instance, filename: str) -> str:
        """Apply ``upload_to`` to ``filename`` and return a relative path."""
        if callable(self.upload_to):
            prefix = self.upload_to(instance, filename) or ""
        else:
            prefix = self.upload_to or ""
        filename = os.path.basename(filename)
        if prefix:
            return f"{str(prefix).strip('/')}/{filename}"
        return filename

    def to_python(self, value: Any) -> Any:
        # Raw DB value → FieldFile wrapping (instance bound lazily in __get__).
        if value is None or isinstance(value, FieldFile):
            return value
        return FieldFile(None, self, str(value))

    def to_db(self, value: Any) -> Optional[str]:
        if value is None:
            return None
        if isinstance(value, FieldFile):
            return value.name
        return str(value)

    def _build_implicit_validators(self) -> None:
        # No NotNull/NotBlank validators — emptiness is handled by null/blank.
        pass

    async def before_save(self, instance: "Model", created: bool) -> None:
        """Commit any staged file content to storage before the SQL write."""
        ff = instance.__dict__.get(self.attname)
        if isinstance(ff, FieldFile) and ff._pending is not None:
            await ff._commit()

    async def after_delete(self, instance: "Model") -> None:
        """Delete the stored file after the row is deleted (best-effort)."""
        ff = instance.__dict__.get(self.attname)
        if isinstance(ff, FieldFile) and ff.name:
            try:
                await ff.storage.delete(ff.name)
            except Exception as e:  # pragma: no cover - best effort
                import logging

                logging.getLogger("ryx.fields").warning(
                    "Could not delete file %s: %s", ff.name, e
                )


####
###     IMAGE FIELD
#####
class ImageField(FileField):
    """A ``FileField`` specialised for images.

    Validation uses Pillow if installed (``pip install ryx[images]``) to verify
    the file is a valid image and to expose :attr:`width` / :attr:`height`.
    Without Pillow, a lightweight magic-bytes check is used.

    Args:
        width_field:  Optional model field name to store the image width.
        height_field: Optional model field name to store the image height.
    """

    def __init__(self, *, width_field: Optional[str] = None, height_field: Optional[str] = None, **kw):
        self.width_field = width_field
        self.height_field = height_field
        super().__init__(**kw)

    async def before_save(self, instance: "Model", created: bool) -> None:
        ff = instance.__dict__.get(self.attname)
        if isinstance(ff, FieldFile) and ff._pending is not None:
            content = ff._pending[1]
            _validate_image_bytes(content)
            dims = _image_dimensions(content)
            if dims is not None:
                w, h = dims
                if self.width_field:
                    setattr(instance, self.width_field, w)
                if self.height_field:
                    setattr(instance, self.height_field, h)
        await super().before_save(instance, created)

    @property
    def width(self) -> Optional[int]:
        return None

    @property
    def height(self) -> Optional[int]:
        return None


_IMAGE_MAGIC: list[tuple[bytes, str]] = [
    (b"\x89PNG\r\n\x1a\n", "png"),
    (b"\xff\xd8\xff", "jpeg"),
    (b"GIF87a", "gif"),
    (b"GIF89a", "gif"),
    (b"RIFF", "webp"),  # WEBP starts with RIFF....WEBP
]


def _validate_image_bytes(content: bytes) -> None:
    """Raise ``ValueError`` if ``content`` is not a recognised image."""
    if not content:
        raise ValueError("Image file is empty")
    for magic, _ in _IMAGE_MAGIC:
        if content.startswith(magic):
            return
    raise ValueError("Uploaded file is not a valid image (unrecognised format)")


def _image_dimensions(content: bytes) -> Optional[tuple[int, int]]:
    """Return ``(width, height)`` using Pillow if available, else ``None``."""
    try:
        import io

        from PIL import Image  # type: ignore

        with Image.open(io.BytesIO(content)) as img:
            return img.size
    except Exception:
        return None


####
###     FOREIGN KEY FIELD
#####
class ForeignKey(Field):
    """Foreign key → stores ``{name}_id`` integer column.

    Args:
        to:              Related model class or string name.
        on_delete:       "CASCADE", "SET_NULL", "PROTECT", "RESTRICT", "SET_DEFAULT".
        related_name:    Name for the reverse relation on the related model.
        db_constraint:   If False, skip the DB FOREIGN KEY constraint (useful for
                          cross-database or legacy schemas).
    """

    def __init__(
        self,
        to: Any,
        *,
        on_delete: str = "CASCADE",
        related_name: Optional[str] = None,
        db_constraint: bool = True,
        **kw,
    ):
        self.to = to
        self.on_delete = on_delete
        self.related_name = related_name
        self.db_constraint = db_constraint
        super().__init__(**kw)

    def contribute_to_class(self, model, name):
        self.attname = f"{name}_id"
        self.column = self._db_column or f"{name}_id"
        self.model = model

        from ryx.descriptors import ForwardDescriptor

        fwd = ForwardDescriptor(self.attname, self.to)
        fwd.__set_name__(model, name)
        type.__setattr__(model, name, fwd)

        rel_name = self.related_name or f"{model.__name__.lower()}_set"
        _pending_reverse_fk.append((self.to, rel_name, model, self.attname))

    def db_type(self) -> str:
        # Inherit the referenced model's primary-key type so a ForeignKey to a
        # UUID (or other) primary key creates a matching column, not INTEGER.
        target = self.to
        if isinstance(target, str):
            from ryx.relations import _resolve_model

            target = _resolve_model(target)
        if target is not None and getattr(target, "_meta", None) is not None:
            pk_field = target._meta.pk_field
            if pk_field is not None:
                return pk_field.db_type()
        return "INTEGER"

    def to_python(self, v):
        # 0 is the SQLite decode of a NULL FK (auto-increment pks start at 1).
        return None if v is None or v == 0 or v == "" else int(v)


####
###     ONE TO ONE FIELD
#####
class OneToOneField(ForeignKey):
    """One-to-one relationship. Same as ForeignKey but adds UNIQUE constraint."""

    def __init__(self, *a, **kw):
        kw.setdefault("unique", True)
        super().__init__(*a, **kw)


####
###     MANY TO MANY FIELD
#####
class ManyToManyField(Field):
    """Many-to-many relationship stub.

    The actual join table is created by the migration system. No column is
    added to the parent table itself.
    """

    def __init__(
        self,
        to: Any,
        *,
        through: Optional[str] = None,
        related_name: Optional[str] = None,
        **kw,
    ):
        self.to = to
        self.through = through
        self.related_name = related_name
        self.attname = ""
        self.column = ""
        self.model = None
        self._validators = []
        self.null = True
        self.blank = True
        self.primary_key = False
        self.unique = False
        self.db_index = False
        self.choices = None
        self.editable = False
        self.help_text = ""
        self.verbose_name = ""
        self._db_column = None
        self.default = _MISSING
        self._join_table = ""
        self._source_fk = ""
        self._target_fk = ""

    def db_type(self) -> str:
        return ""

    def contribute_to_class(self, model, name):
        self.attname = name
        self.model = model

        if hasattr(model, "_meta"):
            model._meta.many_to_many[name] = self

        join_table = self.through or f"{model.__name__.lower()}_{name}"
        source_fk = f"{model.__name__.lower()}_id"
        target_fk = (
            f"{name.removesuffix('s')}_id" if name.endswith("s") else f"{name}_id"
        )

        from ryx.descriptors import ManyToManyDescriptor

        desc = ManyToManyDescriptor(
            target_model_ref=self.to,
            join_table=join_table,
            source_fk=source_fk,
            target_fk=target_fk,
        )
        desc.__set_name__(model, name)
        type.__setattr__(model, name, desc)

        self._join_table = join_table
        self._source_fk = source_fk
        self._target_fk = target_fk

    def _build_implicit_validators(self):
        pass
