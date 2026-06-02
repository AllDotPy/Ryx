use sqlx::Column;

use ryx_query::ast::SqlValue;

use crate::backends::DecodedRow;

#[cfg(feature = "python")]
use ryx_core::model_registry;

pub fn is_date(s: &str) -> bool {
    matches!(s.len(), 10) && s.chars().nth(4) == Some('-') && s.chars().nth(7) == Some('-')
}

pub fn is_timestamp(s: &str) -> bool {
    s.contains(' ') && s.contains('-') && s.contains(':')
}

pub fn decode_rows<T: sqlx::Row>(rows: &[T], base_table: Option<&str>) -> Vec<DecodedRow>
where
    usize: sqlx::ColumnIndex<T>,
    bool: sqlx::Type<T::Database> + for<'r> sqlx::Decode<'r, T::Database>,
    i64: sqlx::Type<T::Database> + for<'r> sqlx::Decode<'r, T::Database>,
    f64: sqlx::Type<T::Database> + for<'r> sqlx::Decode<'r, T::Database>,
    String: sqlx::Type<T::Database> + for<'r> sqlx::Decode<'r, T::Database>,
{
    if rows.is_empty() {
        return Vec::new();
    }

    let col_names: Vec<String> = rows[0]
        .columns()
        .iter()
        .map(|c| c.name().to_string())
        .collect();

    let mapping = std::sync::Arc::new(crate::backends::RowMapping { columns: col_names });

    rows.iter()
        .map(|row| decode_row(row, &mapping, base_table))
        .collect()
}

pub fn decode_row<T: sqlx::Row>(
    row: &T,
    mapping: &std::sync::Arc<crate::backends::RowMapping>,
    base_table: Option<&str>,
) -> DecodedRow
where
    usize: sqlx::ColumnIndex<T>,
    bool: sqlx::Type<T::Database> + for<'r> sqlx::Decode<'r, T::Database>,
    i64: sqlx::Type<T::Database> + for<'r> sqlx::Decode<'r, T::Database>,
    f64: sqlx::Type<T::Database> + for<'r> sqlx::Decode<'r, T::Database>,
    String: sqlx::Type<T::Database> + for<'r> sqlx::Decode<'r, T::Database>,
{
    let mut values = Vec::with_capacity(mapping.columns.len());

    for (idx, name) in mapping.columns.iter().enumerate() {
        let ord = row.columns().get(idx).map(|c| c.ordinal()).unwrap_or(idx);
        let value = lookup_with_spec(row, ord, name, base_table);
        values.push(value);
    }

    crate::backends::RowView {
        values,
        mapping: std::sync::Arc::clone(mapping),
    }
}

/// Try to decode a column using the Python field registry (if available),
/// otherwise fall back to heuristic decoding.
fn lookup_with_spec<T: sqlx::Row>(
    row: &T,
    ord: usize,
    name: &str,
    #[allow(unused)] base_table: Option<&str>,
) -> SqlValue
where
    usize: sqlx::ColumnIndex<T>,
    bool: sqlx::Type<T::Database> + for<'r> sqlx::Decode<'r, T::Database>,
    i64: sqlx::Type<T::Database> + for<'r> sqlx::Decode<'r, T::Database>,
    f64: sqlx::Type<T::Database> + for<'r> sqlx::Decode<'r, T::Database>,
    String: sqlx::Type<T::Database> + for<'r> sqlx::Decode<'r, T::Database>,
{
    #[cfg(feature = "python")]
    {
        if let Some(base_table) = base_table {
            if let Some(spec) = model_registry::lookup_field(base_table, name) {
                return decode_with_spec(row, ord, &spec);
            }
        }
    }
    decode_heuristic(row, ord, name)
}

#[cfg(feature = "python")]
pub fn decode_with_spec<T: sqlx::Row>(
    row: &T,
    ord: usize,
    spec: &model_registry::PyFieldSpec,
) -> SqlValue
where
    usize: sqlx::ColumnIndex<T>,
    bool: sqlx::Type<T::Database> + for<'r> sqlx::Decode<'r, T::Database>,
    i64: sqlx::Type<T::Database> + for<'r> sqlx::Decode<'r, T::Database>,
    f64: sqlx::Type<T::Database> + for<'r> sqlx::Decode<'r, T::Database>,
    String: sqlx::Type<T::Database> + for<'r> sqlx::Decode<'r, T::Database>,
{
    let ty = spec.data_type.as_str();
    match ty {
        "BooleanField" | "NullBooleanField" => row
            .try_get::<bool, _>(ord)
            .map(SqlValue::Bool)
            .unwrap_or(SqlValue::Null),
        "IntegerField" | "BigIntField" | "SmallIntField" | "AutoField" | "BigAutoField"
        | "SmallAutoField" | "PositiveIntField" => row
            .try_get::<i64, _>(ord)
            .map(SqlValue::Int)
            .unwrap_or(SqlValue::Null),
        "FloatField" => row
            .try_get::<f64, _>(ord)
            .map(SqlValue::Float)
            .unwrap_or_else(|_| {
                row.try_get::<String, _>(ord)
                    .map(SqlValue::Text)
                    .unwrap_or(SqlValue::Null)
            }),
        "DecimalField" => row
            .try_get::<String, _>(ord)
            .map(SqlValue::Decimal)
            .unwrap_or(SqlValue::Null),
        "UUIDField" => row
            .try_get::<String, _>(ord)
            .map(SqlValue::Uuid)
            .unwrap_or(SqlValue::Null),
        "CharField" | "TextField" | "SlugField" | "EmailField" | "URLField" => row
            .try_get::<String, _>(ord)
            .map(SqlValue::Text)
            .unwrap_or(SqlValue::Null),
        "DateTimeField" => row
            .try_get::<String, _>(ord)
            .map(SqlValue::DateTime)
            .unwrap_or(SqlValue::Null),
        "DateField" => row
            .try_get::<String, _>(ord)
            .map(SqlValue::Date)
            .unwrap_or(SqlValue::Null),
        "TimeField" => row
            .try_get::<String, _>(ord)
            .map(SqlValue::Time)
            .unwrap_or(SqlValue::Null),
        "JSONField" => row
            .try_get::<String, _>(ord)
            .map(SqlValue::Json)
            .unwrap_or(SqlValue::Null),
        _ => decode_heuristic(row, ord, &spec.name),
    }
}

pub fn decode_heuristic<T: sqlx::Row>(row: &T, column: usize, name: &str) -> SqlValue
where
    usize: sqlx::ColumnIndex<T>,
    bool: sqlx::Type<T::Database> + for<'r> sqlx::Decode<'r, T::Database>,
    i64: sqlx::Type<T::Database> + for<'r> sqlx::Decode<'r, T::Database>,
    f64: sqlx::Type<T::Database> + for<'r> sqlx::Decode<'r, T::Database>,
    String: sqlx::Type<T::Database> + for<'r> sqlx::Decode<'r, T::Database>,
{
    if let Ok(i) = row.try_get::<i64, _>(column) {
        let looks_bool = name.starts_with("is_")
            || name.starts_with("Is_")
            || name.starts_with("IS_")
            || name.starts_with("has_")
            || name.starts_with("Has_")
            || name.starts_with("HAS_")
            || name.starts_with("can_")
            || name.starts_with("Can_")
            || name.starts_with("CAN_")
            || name.ends_with("_flag")
            || name.ends_with("_Flag")
            || name.ends_with("_FLAG");
        if looks_bool && (i == 0 || i == 1) {
            SqlValue::Bool(i != 0)
        } else {
            SqlValue::Int(i)
        }
    } else if let Ok(b) = row.try_get::<bool, _>(column) {
        SqlValue::Bool(b)
    } else if let Ok(f) = row.try_get::<f64, _>(column) {
        SqlValue::Float(f)
    } else if let Ok(s) = row.try_get::<String, _>(column) {
        SqlValue::Text(s)
    } else {
        SqlValue::Null
    }
}
