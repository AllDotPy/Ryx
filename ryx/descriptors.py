"""
Ryx ORM — Relation Descriptors

Provides attribute-level access to related objects on model instances:

  post.author          → Author instance (ForeignKey, lazy-loaded)
  author.posts         → ReverseFKDescriptor (QuerySet-like)
  author.posts.all()   → QuerySet for all posts by this author
  author.posts.filter(active=True) → filtered QuerySet

Design:
  - ForwardDescriptor  : accesses the single related object on the FK side.
    First access triggers a DB query and caches the result on the instance.
  - ReverseFKDescriptor: accessed on the "one" side, returns a bound manager
    that pre-applies a filter for the parent's pk.
  - ManyToManyDescriptor: both sides, returns a M2MManager.

Descriptors are registered by contribute_to_class() at model-build time.
They live on the MODEL class (not on instances) and use __get__ to distinguish
class-level access (return descriptor itself) from instance access.
"""

from __future__ import annotations

from typing import Any, Optional, TYPE_CHECKING

if TYPE_CHECKING:
    from ryx.models import Model


####
##      FORWARD DESCRIPTOR — post.author  →  Author instance
#####
class ForwardDescriptor:
    """Descriptor for the FK owner side: ``post.author`` → Author instance.

    Installed on the model class by :meth:`ForeignKey.contribute_to_class`.
    The descriptor name is the field name *without* the ``_id`` suffix,
    e.g. the field ``author = ForeignKey(...)`` gets both:
      - ``author_id``  — the integer column (managed by the Field descriptor)
      - ``author``     — this ForwardDescriptor (returns a model instance)

    Lazy loading: the related object is fetched on first access and cached
    in ``instance.__dict__["_cache_<name>"]``.
    """

    def __init__(self, field_name: str, related_model_ref: Any) -> None:
        """
        Args:
            field_name:        The FK field attname (e.g. ``"author_id"``).
            related_model_ref: The related model class or a string forward ref.
        """
        self._field_name = field_name      # e.g. "author_id"
        self._related_ref = related_model_ref
        self._attr_name = field_name.removesuffix("_id") if field_name.endswith("_id") else field_name
        self._cache_key = f"_cache_{self._attr_name}"

    def __set_name__(self, owner: type, name: str) -> None:
        self._attr_name = name
        self._cache_key = f"_cache_{name}"

    def __get__(self, instance: Optional["Model"], owner: type) -> Any:
        # Class-level access → return descriptor itself for introspection
        if instance is None:
            return self

        # Check instance cache first (avoid repeated queries)
        cached = instance.__dict__.get(self._cache_key)
        if cached is not None:
            return cached
        if self._cache_key in instance.__dict__:   # explicitly cached as None
            return None

        # Get the FK value
        fk_val = instance.__dict__.get(self._field_name)
        if fk_val is None:
            instance.__dict__[self._cache_key] = None
            return None

        # Resolve model reference (may be a string)
        from ryx.relations import _resolve_model
        related_model = _resolve_model(self._related_ref, type(instance))

        # Lazy load — runs synchronously via run_sync
        from ryx.queryset import run_sync
        from ryx.exceptions import DoesNotExist
        import asyncio

        try:
            loop = asyncio.get_running_loop()
            if loop.is_running():
                # In async context, can't use run_sync with threading due to Tokio runtime issues
                related = related_model(pk=fk_val)
            else:
                related = run_sync(related_model.objects.get(pk=fk_val))
        except DoesNotExist:
            related = None
        except Exception:
            # In running async loop or other concurrency contexts, fall back to
            # a lightweight proxy object with only PK populated.
            try:
                related = related_model(pk=fk_val)
            except Exception:
                related = None

        instance.__dict__[self._cache_key] = related
        return related

    def __set__(self, instance: "Model", value: Any) -> None:
        """Setting ``post.author = author_obj`` updates ``post.author_id``."""
        if value is None:
            instance.__dict__[self._field_name] = None
            instance.__dict__[self._cache_key] = None
            return

        # Accept model instance or plain integer
        from ryx.models import Model as _Model
        if isinstance(value, _Model):
            instance.__dict__[self._field_name] = value.pk
            instance.__dict__[self._cache_key] = value
        else:
            # Assume it's a pk value
            instance.__dict__[self._field_name] = int(value)
            # Invalidate cache when a raw pk is assigned
            instance.__dict__.pop(self._cache_key, None)

    def __delete__(self, instance: "Model") -> None:
        instance.__dict__.pop(self._field_name, None)
        instance.__dict__.pop(self._cache_key, None)


####
##      REVERSE FK MANAGER — author.posts (a bound queryset manager)
#####
class ReverseFKManager:
    """A QuerySet-like manager pre-filtered to a specific parent instance.

    Returned by :class:`ReverseFKDescriptor` when accessed on an instance.

    Usage::

        # author.posts returns a ReverseFKManager
        await author.posts.all()
        await author.posts.filter(active=True)
        await author.posts.count()
        await author.posts.first()

    The manager is lazy — no query is executed until ``await`` or an
    evaluation method is called.
    """

    def __init__(self, child_model: type, fk_field: str, parent_pk: Any) -> None:
        self._child_model = child_model
        self._fk_field    = fk_field      # e.g. "author_id"
        self._parent_pk   = parent_pk

    def _base_qs(self):
        """Return the base QuerySet pre-filtered on the parent PK."""
        return self._child_model.objects.filter(**{self._fk_field: self._parent_pk})

    # Proxy all QuerySet methods
    def all(self): return self._base_qs()
    def filter(self, **kw): return self._base_qs().filter(**kw)
    def exclude(self, **kw): return self._base_qs().exclude(**kw)
    def order_by(self, *f): return self._base_qs().order_by(*f)
    def limit(self, n): return self._base_qs().limit(n)
    def offset(self, n): return self._base_qs().offset(n)
    def distinct(self): return self._base_qs().distinct()
    def annotate(self, **a): return self._base_qs().annotate(**a)
    def values(self, *f): return self._base_qs().values(*f)

    async def count(self) -> int: return await self._base_qs().count()
    async def exists(self) -> bool: return await self._base_qs().exists()
    async def first(self): return await self._base_qs().first()
    async def last(self): return await self._base_qs().last()

    async def get(self, **kw):
        return await self._base_qs().get(**kw)

    async def create(self, **kw):
        """Create a new child object pre-linked to this parent."""
        kw[self._fk_field] = self._parent_pk
        return await self._child_model.objects.create(**kw)

    async def add(self, *instances):
        """Link existing instances to this parent by updating their FK."""
        for inst in instances:
            setattr(inst, self._fk_field, self._parent_pk)
            await inst.save(validate=False, update_fields=[self._fk_field])

    async def remove(self, *instances):
        """Unlink instances by setting their FK to None (null=True required)."""
        for inst in instances:
            setattr(inst, self._fk_field, None)
            await inst.save(validate=False, update_fields=[self._fk_field])

    async def delete(self) -> int:
        """Delete all related objects."""
        return await self._base_qs().delete()

    async def aggregate(self, **aggs):
        return await self._base_qs().aggregate(**aggs)

    def __await__(self):
        return self._base_qs().__await__()

    def __repr__(self) -> str:
        return (
            f"<ReverseFKManager: {self._child_model.__name__}"
            f" where {self._fk_field}={self._parent_pk!r}>"
        )


####
##      REVERSE DESCRIPTOR — installed on Author for ``author.posts``
#####
class ReverseFKDescriptor:
    """Descriptor installed on the parent model to expose the reverse FK.

    Example:
        ``Author.posts`` → descriptor (class-level)
        ``author.posts``  → :class:`ReverseFKManager` bound to ``author.pk``
    """

    def __init__(self, child_model_ref: Any, fk_field: str) -> None:
        self._child_model_ref = child_model_ref   # class or string
        self._fk_field = fk_field           # e.g. "author_id"
        self._attr_name = ""

    def __set_name__(self, owner: type, name: str) -> None:
        self._attr_name = name

    def __get__(self, instance: Optional["Model"], owner: type) -> Any:
        if instance is None:
            return self   # class-level → return descriptor for introspection

        from ryx.relations import _resolve_model
        child_model = _resolve_model(self._child_model_ref, type(instance))

        return ReverseFKManager(
            child_model = child_model,
            fk_field = self._fk_field,
            parent_pk = instance.pk,
        )


####
##      MANY TO MANY MANAGER —  post.tags (a bound M2M manager)
#####
class ManyToManyManager:
    """Manager for many-to-many relationships through a join table.

    Usage::

        await post.tags.all()
        await post.tags.add(tag1, tag2)
        await post.tags.remove(tag1)
        await post.tags.set([tag1, tag2])   # replace entire set
        await post.tags.clear()             # remove all

    The join table is named ``{model_a}_{model_b}`` by convention, or
    explicitly via ``through=`` on the ManyToManyField.
    """

    def __init__(
        self,
        source_model:  type,
        target_model:  type,
        join_table:    str,
        source_fk:     str,   # column in join table pointing to source
        target_fk:     str,   # column in join table pointing to target
        source_pk:     Any,   # pk value of the source instance
    ) -> None:
        self._source_model = source_model
        self._target_model = target_model
        self._join_table   = join_table
        self._source_fk    = source_fk
        self._target_fk    = target_fk
        self._source_pk    = source_pk

    async def all(self) -> list:
        """Return all related target objects."""
        pk_field = self._target_model._meta.pk_field.attname
        pks = await self._get_target_pks()
        if not pks:
            return []
        return await self._target_model.objects.filter(**{f"{pk_field}__in": pks})

    async def add(self, *instances) -> None:
        """Link target instances to this source."""
        from ryx.executor_helpers import raw_execute
        for inst in instances:
            target_pk = inst.pk
            sql = (
                f'INSERT INTO "{self._join_table}" '
                f'("{self._source_fk}", "{self._target_fk}") '
                f'VALUES ({self._source_pk!r}, {target_pk!r})'
            )
            try:
                await raw_execute(sql)
            except Exception:
                pass  # ignore duplicate key errors (already linked)

    async def remove(self, *instances) -> None:
        """Unlink target instances from this source."""
        from ryx.executor_helpers import raw_execute
        for inst in instances:
            target_pk = inst.pk
            sql = (
                f'DELETE FROM "{self._join_table}" '
                f'WHERE "{self._source_fk}" = {self._source_pk!r} '
                f'AND "{self._target_fk}" = {target_pk!r}'
            )
            await raw_execute(sql)

    async def set(self, instances: list) -> None:
        """Replace the entire set of linked objects."""
        await self.clear()
        if instances:
            await self.add(*instances)

    async def clear(self) -> None:
        """Remove all links from this source."""
        from ryx.executor_helpers import raw_execute
        sql = (
            f'DELETE FROM "{self._join_table}" '
            f'WHERE "{self._source_fk}" = {self._source_pk!r}'
        )
        await raw_execute(sql)

    async def count(self) -> int:
        """Count linked target objects."""
        pks = await self._get_target_pks()
        return len(pks)

    async def exists(self) -> bool:
        return await self.count() > 0

    async def _get_target_pks(self) -> list:
        """Fetch all target PKs from the join table."""
        from ryx.executor_helpers import raw_fetch
        sql = (
            f'SELECT "{self._target_fk}" FROM "{self._join_table}" '
            f'WHERE "{self._source_fk}" = {self._source_pk!r}'
        )
        rows = await raw_fetch(sql)
        return [r[self._target_fk] for r in rows]

    def __await__(self):
        return self.all().__await__()

    def __repr__(self) -> str:
        return (
            f"<ManyToManyManager: {self._source_model.__name__}"
            f" ↔ {self._target_model.__name__} via {self._join_table!r}>"
        )


####
##      MANY TO MANY DESCRIPTOR
#####
class ManyToManyDescriptor:
    """Descriptor installed on both sides of a ManyToMany relationship."""

    def __init__(
        self,
        target_model_ref: Any,
        join_table: str,
        source_fk: str,
        target_fk: str,
    ) -> None:
        self._target_ref = target_model_ref
        self._join_table = join_table
        self._source_fk = source_fk
        self._target_fk = target_fk
        self._attr_name = ""

    def __set_name__(self, owner: type, name: str) -> None:
        """Called by Python metaclass machinery when installed on a class."""
        self._attr_name = name

    def __get__(self, instance: Optional["Model"], owner: type) -> Any:
        if instance is None:
            return self
        from ryx.relations import _resolve_model
        target_model = _resolve_model(self._target_ref, type(instance))
        return ManyToManyManager(
            source_model = type(instance),
            target_model = target_model,
            join_table = self._join_table,
            source_fk = self._source_fk,
            target_fk = self._target_fk,
            source_pk = instance.pk,
        )