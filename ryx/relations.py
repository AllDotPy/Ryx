"""
Ryx ORM — Related Object Loading

Implements two strategies for loading related objects:

1. select_related(fields)  — LEFT JOIN + single query (1 SQL hit)
   Best for: ForeignKey / OneToOne where most rows have a related object.
   Attaches the related object directly as an attribute on each instance.

2. prefetch_related(fields) — N+1 turned into 2 queries per relation
   Best for: ManyToMany / reverse ForeignKey / large result sets.
   Fetches all related objects in one IN query, then distributes them.

Usage (via QuerySet):
  posts = await Post.objects.select_related("author").filter(active=True)
  # → posts[0].author is an Author instance (no extra queries)

  posts = await Post.objects.prefetch_related("tags").filter(active=True)
  # → posts[0].tags is a list of Tag instances (fetched in 1 extra query)

Design notes:
  - select_related uses QueryBuilder.add_join() which produces a LEFT OUTER
    JOIN. The Rust executor returns flat rows; we reconstruct model instances
    by splitting row keys on the relation prefix.
  - prefetch_related runs after the main query is evaluated. It collects all
    FK values from the result set and fires a single `pk__in` query.
  - Both methods are non-destructive: they return new QuerySet instances.
"""

from __future__ import annotations

# import asyncio
from typing import Any, Dict, List, TYPE_CHECKING

if TYPE_CHECKING:
    from ryx.models import Model
    from ryx.queryset import QuerySet


####    select_related implementation
async def apply_select_related(
    qs: "QuerySet",
    fields: List[str],
) -> List["Model"]:
    """Execute a SELECT with LEFT JOINs for each related field.

    For each field name in ``fields``:
      1. Looks up the ForeignKey declaration on the model.
      2. Resolves the related model class and table name.
      3. Adds a LEFT OUTER JOIN on ``{parent_table}.{fk_col} = {rel_table}.id``.
      4. Selects all columns from both tables (prefixed to avoid collisions).
      5. Reconstructs both model instances from the flat row.

    Args:
        qs:     The base QuerySet to augment.
        fields: List of ForeignKey field names to JOIN in.

    Returns:
        List of model instances with related objects pre-loaded as attributes.
    """

    model = qs._model
    builder = qs._builder

    # Track which related models we've joined and their column prefix
    joins: Dict[str, type] = {}  # field_name → related_model_class

    for field_name in fields:
        if field_name not in model._meta.fields:
            raise ValueError(
                f"{model.__name__} has no field '{field_name}'. "
                f"Available fields: {list(model._meta.fields.keys())}"
            )
        
        field = model._meta.fields[field_name]
        from ryx.fields import ForeignKey, OneToOneField
        if not isinstance(field, (ForeignKey, OneToOneField)):
            raise TypeError(
                f"select_related only works with ForeignKey/OneToOneField. "
                f"'{field_name}' is {type(field).__name__}."
            )

        # Resolve related model class (handle string forward references)
        related_model = _resolve_model(field.to, model)
        related_table = related_model._meta.table_name
        alias = f"_sr_{field_name}"  # unique alias per join

        # Add LEFT OUTER JOIN
        # ON: parent_table.author_id = _sr_author.id
        pk_col = related_model._meta.pk_field.column if related_model._meta.pk_field else "id"
        builder = builder.add_join(
            "LEFT",
            related_table,
            alias,
            f"{model._meta.table_name}.{field.column}",  # e.g. posts.author_id
            f"{alias}.{pk_col}",                          # e.g. _sr_author.id
        )
        joins[field_name] = related_model

    # Execute the query
    raw_rows = await builder.fetch_all()

    # Reconstruct instances
    result: List[Model] = []
    for row in raw_rows:
        # Main model row (columns without a prefix)
        main_row = {k: v for k, v in row.items() if not k.startswith("_sr_")}
        instance = model._from_row(main_row)

        # Related model rows (columns prefixed with _sr_{field_name}__)
        for field_name, related_model in joins.items():
            prefix = f"_sr_{field_name}__"
            rel_row = {
                k[len(prefix):]: v
                for k, v in row.items()
                if k.startswith(prefix)
            }
            if rel_row and any(v is not None for v in rel_row.values()):
                rel_instance = related_model._from_row(rel_row)
            else:
                rel_instance = None
            # Attach as attribute e.g. post.author = <Author pk=1>
            # Use the field name without _id suffix
            attr_name = field_name.removesuffix("_id") if field_name.endswith("_id") else field_name
            object.__setattr__(instance, attr_name, rel_instance)

        result.append(instance)

    return result


####    prefetch_related implementation
async def apply_prefetch_related(
    instances: List["Model"],
    field_names: List[str],
) -> List["Model"]:
    """Fetch related objects for a list of already-loaded model instances.

    For each field name:
      1. Collect all FK values from the instances.
      2. Fire a single ``pk__in=[...]`` query against the related table.
      3. Build a dict mapping FK value → related instance(s).
      4. Attach the related instance(s) to each parent instance.

    For ForeignKey (many→one) the attribute is set to the single related object.
    For reverse FK / ManyToMany the attribute is set to a list.

    Args:
        instances:   The parent model instances (already loaded).
        field_names: Related field names to prefetch.

    Returns:
        The same instances list with related attributes attached in-place.
    """
    if not instances:
        return instances

    model = instances[0].__class__

    for field_name in field_names:
        await _prefetch_one(instances, model, field_name)

    return instances


async def _prefetch_one(
    instances: List["Model"],
    model: type,
    field_name: str,
) -> None:
    """Prefetch a single relation onto the given instances."""
    from ryx.fields import ForeignKey, OneToOneField

    if field_name not in model._meta.fields:
        raise ValueError(
            f"{model.__name__} has no field '{field_name}'. "
            f"Available: {list(model._meta.fields.keys())}"
        )

    field = model._meta.fields[field_name]

    if not isinstance(field, (ForeignKey, OneToOneField)):
        raise TypeError(
            f"prefetch_related only supports ForeignKey/OneToOneField for now. "
            f"'{field_name}' is {type(field).__name__}."
        )

    # Collect FK values (deduplicated, no None)
    fk_attr = field.attname  # e.g. "author_id"
    fk_values = list({
        getattr(inst, fk_attr)
        for inst in instances
        if getattr(inst, fk_attr) is not None
    })

    if not fk_values:
        # No FK values → nothing to prefetch
        attr_name = field_name.removesuffix("_id") if field_name.endswith("_id") else field_name
        for inst in instances:
            object.__setattr__(inst, attr_name, None)
        return

    # Resolve related model
    related_model = _resolve_model(field.to, model)
    pk_col = related_model._meta.pk_field.attname if related_model._meta.pk_field else "id"

    # Single IN query for all FK values
    related_objects = await related_model.objects.filter(**{f"{pk_col}__in": fk_values})

    # Build lookup dict: pk → instance
    pk_map = {getattr(obj, pk_col): obj for obj in related_objects}

    # Attach to parent instances
    attr_name = field_name.removesuffix("_id") if field_name.endswith("_id") else field_name
    for inst in instances:
        fk_val = getattr(inst, fk_attr)
        object.__setattr__(inst, attr_name, pk_map.get(fk_val))



####    Helper: resolve model class from string or class reference
def _resolve_model(to: Any, source_model: type) -> type:
    """Resolve a ForeignKey target to an actual model class.

    Handles:
      - Already a class  → return as-is
      - String name      → look up in the same module as source_model
      - "self"           → return source_model itself (self-referential FK)
    """

    if isinstance(to, type):
        return to

    if isinstance(to, str):
        if to.lower() == "self":
            return source_model

        # Search in the source model's module
        import sys
        module = sys.modules.get(source_model.__module__)
        if module and hasattr(module, to):
            return getattr(module, to)

        # Fall through to a helpful error
        raise ValueError(
            f"Cannot resolve ForeignKey target '{to}'. "
            f"Make sure the model class is defined in the same module as {source_model.__name__}, "
            f"or pass the class directly instead of a string."
        )

    raise TypeError(
        f"ForeignKey 'to' must be a Model class or a string. Got: {type(to).__name__}"
    )
