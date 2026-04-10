"""
Ryx ORM — Model Base Class

The Model class is the heart of the Ryx ORM. It provides:
  Meta options:
    unique_together   : list[tuple[str,...]] — multi-column uniqueness
    index_together    : list[tuple[str,...]] — multi-column indexes
    indexes           : list[Index]          — named index declarations
    ordering          : list[str]            — default ORDER BY
    constraints       : list[Constraint]     — named constraints

  Per-instance hooks (override in subclass):
    async def clean(self)                → model-level validation
    async def before_save(self, created) → pre-SQL hook
    async def after_save(self,  created) → post-SQL hook
    async def before_delete(self)        → pre-SQL hook
    async def after_delete(self)         → post-SQL hook

  Global signals (fired automatically):
    pre_save, post_save, pre_delete, post_delete

  Validation:
    await instance.full_clean()   → runs validators + clean()
    model.save(validate=True)     → calls full_clean() before SQL (default)
"""

from __future__ import annotations

import re
from datetime import datetime
from typing import Any, Dict, List, Optional

from ryx import ryx_core as _core
from ryx.exceptions import DoesNotExist, MultipleObjectsReturned
from ryx.fields import AutoField, DateTimeField, Field, ManyToManyField
from ryx.signals import post_delete, post_save, pre_delete, pre_save
from ryx.validators import ValidationError, run_full_validation


####
##      INDEX AND CONSTRANT DESCRIPTORS (used in Meta)
#####
class Index:
    """Declares a database index on one or more columns.

    Usage (in Meta)::

        class Meta:
            indexes = [
                Index(fields=["title"], name="post_title_idx"),
                Index(fields=["author_id", "created_at"], name="post_author_date_idx"),
                Index(fields=["title"], name="post_title_unique_idx", unique=True),
            ]
    """

    def __init__(self, *, fields: List[str], name: str, unique: bool = False) -> None:
        self.fields = fields
        self.name = name
        self.unique = unique

    def __repr__(self) -> str:
        return f"<Index: {self.name} on {self.fields}>"


####
##      CONTRAINT DESCRIPTOR
#####
class Constraint:
    """Declares a named database constraint.

    Usage (in Meta)::

        class Meta:
            constraints = [
                Constraint(check="views >= 0", name="posts_views_positive"),
            ]
    """

    def __init__(self, *, check: str, name: str) -> None:
        self.check = check
        self.name = name

    def __repr__(self) -> str:
        return f"<Constraint: {self.name}>"


####
##      MODEL META OPTIONS CLASS (_meta)
#####
class Options:
    """Model metadata — ``Model._meta``.

    Attributes:
        table_name       : SQL table name.
        app_label        : Optional namespace prefix.
        database         : Optional database alias (e.g. "logs").
        fields           : Ordered dict name → Field.
        many_to_many     : Dict name → ManyToManyField (populated by M2M fields).
        pk_field         : The primary key Field.
        ordering         : Default ORDER BY (list of "-field" / "field").
        unique_together  : Multi-column uniqueness constraints.
        index_together   : Multi-column indexes (legacy — prefer ``indexes``).
        indexes          : Named Index declarations.
        constraints      : Named Constraint declarations.
        abstract         : If True, no table is created; fields are inherited.
        managed          : If False, Ryx will never CREATE/DROP this table.
    """

    def __init__(self, meta_class: Optional[type], model_name: str) -> None:
        # Table name
        if meta_class and hasattr(meta_class, "table_name"):
            self.table_name: str = meta_class.table_name
        else:
            self.table_name = _to_table_name(model_name)

        self.app_label: str = getattr(meta_class, "app_label", "")
        self.database: Optional[str] = getattr(meta_class, "database", None)
        self.ordering: List[str] = list(getattr(meta_class, "ordering", []))

        self.unique_together: List[tuple] = list(
            getattr(meta_class, "unique_together", [])
        )
        self.index_together: List[tuple] = list(
            getattr(meta_class, "index_together", [])
        )
        self.indexes: List[Index] = list(getattr(meta_class, "indexes", []))
        self.constraints: List[Constraint] = list(
            getattr(meta_class, "constraints", [])
        )
        self.abstract: bool = getattr(meta_class, "abstract", False)
        self.managed: bool = getattr(meta_class, "managed", True)

        # Populated by metaclass
        self.fields: Dict[str, Field] = {}
        self.many_to_many: Dict[str, ManyToManyField] = {}
        self.pk_field: Optional[Field] = None

    def add_field(self, field: Field) -> None:
        if not field.column:  # M2M fields have no column
            return
        self.fields[field.attname] = field
        if field.primary_key:
            self.pk_field = field

    @property
    def field_names(self) -> List[str]:
        return list(self.fields.keys())

    @property
    def column_names(self) -> List[str]:
        return [f.column for f in self.fields.values()]

    def get_field(self, name: str) -> Field:
        return self.fields[name]


####
###     MODEL MANAGER
#####
class Manager:
    """Default query manager. Proxies to QuerySet."""

    def __init__(self, alias: Optional[str] = None) -> None:
        self._model: Optional[type[Model]] = None
        self._alias = alias

    def contribute_to_class(self, model: type, name: str) -> None:
        self._model = model

    def get_queryset(self):
        from ryx.queryset import QuerySet

        return QuerySet(self._model, _using=self._alias)

    # Proxy shortcuts
    def all(self):
        return self.get_queryset()

    def filter(self, *q, **kw):
        return self.get_queryset().filter(*q, **kw)

    def exclude(self, *q, **kw):
        return self.get_queryset().exclude(*q, **kw)

    def order_by(self, *f):
        return self.get_queryset().order_by(*f)

    def using(self, alias: str) -> "Manager":
        """Return a new Manager bound to the specified database alias."""
        new_mgr = Manager()
        new_mgr._model = self._model
        new_mgr._alias = alias
        return new_mgr

    def cache(self, **kw):
        return self.get_queryset().cache(**kw)

    def annotate(self, **aggs):
        return self.get_queryset().annotate(**aggs)

    def values(self, *fields):
        return self.get_queryset().values(*fields)

    def join(self, *a, **kw):
        return self.get_queryset().join(*a, **kw)

    def distinct(self):
        return self.get_queryset().distinct()

    def select_related(self, *f):
        return self.get_queryset().select_related(*f)

    def stream(self, **kw):
        return self.get_queryset().stream(**kw)

    async def aggregate(self, **aggs):
        return await self.get_queryset().aggregate(**aggs)

    async def get(self, **kw):
        return await self.get_queryset().get(**kw)

    async def first(self):
        return await self.get_queryset().first()

    async def last(self):
        return await self.get_queryset().last()

    async def exists(self) -> bool:
        return await self.get_queryset().exists()

    async def count(self) -> int:
        return await self.get_queryset().count()

    async def create(self, **kw):
        """Create and save a new model instance."""
        instance = self._model(**kw)

        # Use the manager's alias if specified
        from ryx.router import get_router

        router = get_router()
        alias = None
        if router:
            alias = router.db_for_write(self._model)
        if not alias:
            alias = self._model._meta.database
        if not alias:
            alias = self._alias

        # We need a way to pass the alias to instance.save()
        # Let's add an optional `using` argument to save()
        await instance.save(using=alias)
        return instance

    async def get_or_create(self, defaults: Optional[dict] = None, **kw):
        """Return (instance, created). created=True if a new row was inserted."""
        try:
            obj = await self.get(**kw)
            return obj, False
        except self._model.DoesNotExist:
            params = {**kw, **(defaults or {})}
            obj = await self.create(**params)
            return obj, True

    async def update_or_create(self, defaults: Optional[dict] = None, **kw):
        """Return (instance, created). Update existing or create new."""
        defaults = defaults or {}
        try:
            obj = await self.get(**kw)
            for attr, val in defaults.items():
                setattr(obj, attr, val)
            await obj.save()
            return obj, False
        except self._model.DoesNotExist:
            params = {**kw, **defaults}
            obj = await self.create(**params)
            return obj, True

    async def bulk_create(self, instances: list[Model], batch_size: int = 500) -> list:
        """Insert many instances in batches using multi-row INSERT.

        Returns the list with PKs set (if the DB supports RETURNING).
        Delegates to the optimized ``ryx.bulk.bulk_create`` function.
        """
        from ryx.bulk import bulk_create

        return await bulk_create(self._model, instances, batch_size=batch_size)

    async def bulk_update(
        self, instances: list, fields: list, batch_size: int = 500
    ) -> int:
        from ryx.bulk import bulk_update as _update

        return await _update(self._model, instances, fields, batch_size=batch_size)

    async def bulk_delete(
        self, instances: Optional[list] = None, batch_size: int = 500
    ) -> int:
        """Delete many instances. If no instances given, delete all."""
        if instances is None:
            return await self.get_queryset().delete()
        from ryx.bulk import bulk_delete

        return await bulk_delete(self._model, instances, batch_size=batch_size)

        return await bulk_delete(self._model, instances)


####
###     MODEL META CLASS
#####
class ModelMetaclass(type):
    """Processes Model subclass definitions.

    Steps:
      1. Extract inner ``Meta`` class and build ``Options``.
      2. Collect ``Field`` declarations (including inherited ones).
      3. Add implicit ``id = AutoField()`` if no PK declared.
      4. Call ``field.contribute_to_class()`` on each field.
      5. Inject per-model ``DoesNotExist`` / ``MultipleObjectsReturned``.
      6. Attach default ``objects`` Manager.
    """

    def __new__(mcs, name: str, bases: tuple, namespace: dict, **kw) -> type:
        # Guard: short-circuit for the root Model class itself.
        # We use _ryx_model_class as sentinel because Model has no _meta
        # (it is only set on subclasses by this very metaclass). Using _meta
        # as the guard would cause ALL subclasses to be skipped too.
        if not any(getattr(b, "_ryx_model_class", False) for b in bases):
            cls = super().__new__(mcs, name, bases, namespace)
            cls._ryx_model_class = True  # mark Model itself as the root
            return cls

        meta_class = namespace.pop("Meta", None)
        opts = Options(meta_class, name)

        # Collect fields
        fields: Dict[str, Field] = {}

        # Inherit from base models (MRO order, reversed so child wins)
        for base in reversed(bases):
            if hasattr(base, "_meta"):
                for fn, f in base._meta.fields.items():
                    fields[fn] = f

        # Fields declared in this class
        for attr, val in list(namespace.items()):
            if isinstance(val, (Field, ManyToManyField)):
                fields[attr] = val

        # Implicit AutoField
        if not opts.abstract:
            has_pk = any(
                f.primary_key
                for f in fields.values()
                if not isinstance(f, ManyToManyField)
            )
            if not has_pk:
                auto = AutoField(primary_key=True, editable=False)
                namespace["id"] = auto
                fields = {"id": auto, **fields}

        # Create class
        cls = super().__new__(mcs, name, bases, namespace)
        cls._meta = opts

        for fn, field in fields.items():
            field.contribute_to_class(cls, fn)
            opts.add_field(field)

        # Per-model exception classes
        cls.DoesNotExist = type(
            f"{name}.DoesNotExist",
            (DoesNotExist,),
            {"__module__": namespace.get("__module__", "")},
        )
        cls.MultipleObjectsReturned = type(
            f"{name}.MultipleObjectsReturned",
            (MultipleObjectsReturned,),
            {"__module__": namespace.get("__module__", "")},
        )

        # Default manager
        if "objects" not in namespace:
            mgr = Manager()
            mgr.contribute_to_class(cls, "objects")
            cls.objects = mgr

        # Resolve pending reverse FK descriptors
        # ForeignKey fields may carry string forward references that could
        # not resolve immediately. Now that this model exists, retry.
        try:
            from ryx.fields import resolve_pending_reverse_fks

            resolve_pending_reverse_fks()
        except Exception:
            pass  # never let descriptor resolution crash model creation

        return cls


####
###     MODEL CLASS
#####
class Model(metaclass=ModelMetaclass):
    """Base class for all Ryx database models.

    Hooks
    -----
    Override these async methods in your subclass::

        async def clean(self):
            \"\"\"Cross-field validation. Raise ValidationError on failure.\"\"\"

        async def before_save(self, created: bool) -> None:
            \"\"\"Called before INSERT or UPDATE (after validation).\"\"\"

        async def after_save(self, created: bool) -> None:
            \"\"\"Called after INSERT or UPDATE.\"\"\"

        async def before_delete(self) -> None:
            \"\"\"Called before DELETE.\"\"\"

        async def after_delete(self) -> None:
            \"\"\"Called after DELETE.\"\"\"

    Meta options
    ------------
    ::

        class Meta:
            table_name = "my_table"
            ordering = ["-created_at"]
            unique_together = [("author", "slug")]
            index_together = [("author", "created_at")]
            indexes = [Index(fields=["title"], name="idx_title")]
            constraints = [Constraint(check="views >= 0", name="chk_views")]
            abstract = False
            managed = True
    """

    _meta: Options
    objects: Manager

    def __init__(self, **kwargs: Any) -> None:
        # Set field defaults first
        for field in self._meta.fields.values():
            object.__setattr__(self, field.attname, field.get_default())

        # Apply user-provided values
        for key, val in kwargs.items():
            if key == "pk" and self._meta.pk_field:
                key = self._meta.pk_field.attname

            if key not in self._meta.fields:
                # Allow setting forward relationship fields directly (e.g. author=Author(...))
                if hasattr(type(self), key):
                    setattr(self, key, val)
                    continue
                raise TypeError(
                    f"{type(self).__name__}() got unexpected keyword argument {key!r}"
                )

            setattr(self, key, val)

    # Class method: build from raw DB row
    @classmethod
    def _from_row(cls, row: dict) -> "Model":
        """Build a model instance from a raw decoded DB row (no validation)."""

        instance = cls.__new__(cls)
        for field in cls._meta.fields.values():
            object.__setattr__(instance, field.attname, field.get_default())

        for field in cls._meta.fields.values():
            if field.column in row:
                object.__setattr__(
                    instance, field.attname, field.to_python(row[field.column])
                )
        return instance

    # Properties
    @property
    def pk(self) -> Any:
        if self._meta.pk_field:
            return getattr(self, self._meta.pk_field.attname, None)
        return None

    # Hooks (no-ops by default — override in subclass)
    async def clean(self) -> None:
        """Override to add model-level (cross-field) validation.

        Raise ``ValidationError`` to signal invalid state::

            async def clean(self):
                if self.end_date < self.start_date:
                    raise ValidationError({"end_date": ["Must be after start date"]})
        """

    async def before_save(self, created: bool) -> None:
        """Called before the INSERT or UPDATE SQL is executed.

        Args:
            created: True on INSERT, False on UPDATE.
        """

    async def after_save(self, created: bool) -> None:
        """Called after the INSERT or UPDATE SQL is executed (and pk is set)."""

    async def before_delete(self) -> None:
        """Called before the DELETE SQL is executed."""

    async def after_delete(self) -> None:
        """Called after the DELETE SQL is executed (pk is None at this point)."""

    # Validation

    async def full_clean(self) -> None:
        """Run all field validators + model.clean().

        Raises:
            ValidationError: collected from all fields and clean().
        """
        await run_full_validation(self)

    # Persistence
    async def save(
        self,
        *,
        validate: bool = True,
        update_fields: Optional[List[str]] = None,
        using: Optional[str] = None,
    ) -> None:
        """Save the instance to the database.

        - First run INSERT (if pk is None), otherwise UPDATE.
        - Fires hooks and signals in order.
        - Runs full_clean() by default (pass ``validate=False`` to skip).

        Args:
            validate:      Run field validators + clean() before SQL (default: True).
            update_fields: If given, only UPDATE these field names (reduces SQL chatter).
            using:         Explicitly specify the database alias to use.
        """
        created = self.pk is None

        # auto_now / auto_now_add
        _apply_auto_timestamps(self, created)

        # Validation
        if validate:
            await self.full_clean()

        # before_save hook
        await self.before_save(created)

        # pre_save signal
        await pre_save.send(sender=type(self), instance=self, created=created)

        # Resolve database alias: using -> Router.db_for_write -> Meta.database -> 'default'
        from ryx.router import get_router

        router = get_router()
        alias = using
        if not alias:
            if router:
                alias = router.db_for_write(type(self))
            if not alias:
                alias = self._meta.database

        # SQL execution
        # Creation
        if created:
            fields_to_save = [
                f
                for f in self._meta.fields.values()
                if not f.primary_key
                and (f.editable or getattr(f, "auto_now_add", False))
            ]
            values = [
                (f.column, f.to_db(getattr(self, f.attname))) for f in fields_to_save
            ]
            builder = _core.QueryBuilder(self._meta.table_name)
            if alias:
                builder = builder.set_using(alias)
            new_id = await builder.execute_insert(values, returning_id=True)
            if self._meta.pk_field:
                object.__setattr__(self, self._meta.pk_field.attname, new_id)

        # Update
        else:
            if update_fields:
                fields_to_save = [
                    f
                    for f in self._meta.fields.values()
                    if f.attname in update_fields and not f.primary_key
                ]
            else:
                fields_to_save = [
                    f
                    for f in self._meta.fields.values()
                    if not f.primary_key
                    and (f.editable or getattr(f, "auto_now", False))
                ]
            values = [
                (f.column, f.to_db(getattr(self, f.attname))) for f in fields_to_save
            ]
            pk_field = self._meta.pk_field
            builder = _core.QueryBuilder(self._meta.table_name)
            if alias:
                builder = builder.set_using(alias)
            builder = builder.add_filter(
                pk_field.column, "exact", self.pk, negated=False
            )
            await builder.execute_update(values)

        # after_save hook
        await self.after_save(created)

        # post_save signal
        await post_save.send(sender=type(self), instance=self, created=created)

    async def delete(self) -> None:
        """Delete this instance from the database.

        Raises:
            RuntimeError: if the instance has no pk (was never saved).
        """
        if self.pk is None:
            raise RuntimeError(
                f"Cannot delete an unsaved {type(self).__name__} instance."
            )

        await self.before_delete()
        await pre_delete.send(sender=type(self), instance=self)

        # Resolve database alias: Router.db_for_write -> Meta.database -> 'default'
        from ryx.router import get_router

        router = get_router()
        alias = None
        if router:
            alias = router.db_for_write(type(self))
        if not alias:
            alias = self._meta.database

        from ryx import ryx_core as _core

        pk_field = self._meta.pk_field
        builder = _core.QueryBuilder(self._meta.table_name)
        if alias:
            builder = builder.set_using(alias)
        builder = builder.add_filter(pk_field.column, "exact", self.pk, negated=False)
        await builder.execute_delete()

        # Clear pk to signal "no longer in DB"
        object.__setattr__(self, self._meta.pk_field.attname, None)

        await self.after_delete()
        await post_delete.send(sender=type(self), instance=self)

    async def refresh_from_db(self, fields: Optional[List[str]] = None) -> None:
        """Reload this instance's fields from the database.

        Args:
            fields: If given, reload only these field names.
                    If None, reload all fields.
        """
        if self.pk is None:
            raise RuntimeError("Cannot refresh an unsaved instance.")
        fresh = await type(self).objects.get(pk=self.pk)
        reload_fields = fields or list(self._meta.fields.keys())
        for fname in reload_fields:
            object.__setattr__(self, fname, getattr(fresh, fname))

    # Utility
    def __repr__(self) -> str:
        return f"<{type(self).__name__}: pk={self.pk!r}>"

    def __str__(self) -> str:
        return repr(self)

    def __eq__(self, other: object) -> bool:
        if not isinstance(other, type(self)):
            return NotImplemented
        return self.pk is not None and self.pk == other.pk

    def __hash__(self) -> int:
        return hash((type(self), self.pk))


####    Helpers
def _to_table_name(class_name: str) -> str:
    """CamelCase → snake_case plural."""
    snake = re.sub(r"(?<!^)(?=[A-Z][a-z])", "_", class_name).lower()
    if snake.endswith(("s", "x", "z", "ch", "sh")):
        return snake + "es"
    return snake + "s"


def _apply_auto_timestamps(instance: Model, created: bool) -> None:
    """Set auto_now / auto_now_add DateTimeField values before saving."""
    now = datetime.utcnow()
    for field in instance._meta.fields.values():
        if isinstance(field, DateTimeField):
            if field.auto_now:
                object.__setattr__(instance, field.attname, now)
            elif field.auto_now_add and created:
                object.__setattr__(instance, field.attname, now)
