use ryx_common::SqlValue;

pub trait IntoSqlValue {
    fn into_sql_value(self) -> SqlValue;
}

// Primitives
impl IntoSqlValue for i32 {
    fn into_sql_value(self) -> SqlValue {
        SqlValue::Int(self as i64)
    }
}

impl IntoSqlValue for i64 {
    fn into_sql_value(self) -> SqlValue {
        SqlValue::Int(self)
    }
}

impl IntoSqlValue for f64 {
    fn into_sql_value(self) -> SqlValue {
        SqlValue::Float(self)
    }
}

impl IntoSqlValue for bool {
    fn into_sql_value(self) -> SqlValue {
        SqlValue::Bool(self)
    }
}

impl IntoSqlValue for String {
    fn into_sql_value(self) -> SqlValue {
        SqlValue::Text(self)
    }
}

impl IntoSqlValue for &str {
    fn into_sql_value(self) -> SqlValue {
        SqlValue::Text(self.to_string())
    }
}

// Option<T>
impl<T: IntoSqlValue> IntoSqlValue for Option<T> {
    fn into_sql_value(self) -> SqlValue {
        match self {
            Some(v) => v.into_sql_value(),
            None => SqlValue::Null,
        }
    }
}

// Chrono types
impl IntoSqlValue for chrono::NaiveDateTime {
    fn into_sql_value(self) -> SqlValue {
        SqlValue::DateTime(self.format("%Y-%m-%d %H:%M:%S").to_string())
    }
}

impl IntoSqlValue for chrono::NaiveDate {
    fn into_sql_value(self) -> SqlValue {
        SqlValue::Date(self.format("%Y-%m-%d").to_string())
    }
}

// Vec<T> for IN queries
impl<T: IntoSqlValue> IntoSqlValue for Vec<T> {
    fn into_sql_value(self) -> SqlValue {
        SqlValue::List(
            self.into_iter()
                .map(|v| Box::new(v.into_sql_value()))
                .collect(),
        )
    }
}
