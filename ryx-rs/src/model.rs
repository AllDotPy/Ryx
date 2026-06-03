use async_trait::async_trait;

/// Metadata for a single database column, used by the migration system.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldMeta {
    pub name: &'static str,
    pub db_type: &'static str,
    pub nullable: bool,
    pub primary_key: bool,
    pub unique: bool,
    pub default: Option<&'static str>,
}

/// Trait representing a database model.
///
/// Automatically derived via `#[derive(Model)]` or `#[model]`.
#[async_trait]
pub trait Model: Send + Sync + 'static {
    fn table_name() -> &'static str;
    fn field_names() -> &'static [&'static str];
    fn pk_field() -> &'static str;
    /// Return metadata for every column in the table.
    ///
    /// Used by the migration system to compare the model's schema
    /// against the live database.
    fn field_meta() -> &'static [FieldMeta];
    /// Database alias this model belongs to — used by the migration system
    /// to route operations to the correct database.
    ///
    /// Set via `#[database("blog")]` on the struct or `#[model(database = "blog")]`.
    /// Defaults to `"default"`.
    fn database() -> &'static str {
        "default"
    }
}

/// Metadata for a foreign-key relationship.
///
/// Used by `select_related()` and `prefetch_related()` to generate
/// JOIN clauses and query related rows.
///
/// # Example
///
/// ```ignore
/// impl Relationships for Post {
///     fn relations() -> &'static [RelationMeta] {
///         &[RelationMeta {
///             name: "author",
///             fk_column: "author_id",
///             to_table: "authors",
///             to_field: "id",
///         }]
///     }
/// }
/// ```
#[derive(Debug, Clone)]
pub struct RelationMeta {
    /// Name used in `select_related("author")`
    pub name: &'static str,
    /// FK column on this table (e.g., `"author_id"`)
    pub fk_column: &'static str,
    /// Related table (e.g., `"authors"`)
    pub to_table: &'static str,
    /// PK column on the related table (e.g., `"id"`)
    pub to_field: &'static str,
    /// Column names of the related model (used by select_related for column aliasing)
    pub relation_fields: &'static [&'static str],
}

/// Optional trait for models that have foreign-key relationships.
///
/// Implement this to enable `select_related()` and `prefetch_related()`.
pub trait Relationships: Model {
    fn relations() -> &'static [RelationMeta];
}
