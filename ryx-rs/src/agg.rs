use ryx_query::ast::{AggFunc, AggregateExpr};
use ryx_query::symbols::Symbol;

/// An aggregate expression for use with `.aggregate()`.
#[derive(Clone)]
pub struct AggExpr {
    pub alias: String,
    pub func: AggFunc,
    pub field: String,
    pub distinct: bool,
}

impl AggExpr {
    pub fn into_ast(self) -> AggregateExpr {
        AggregateExpr {
            alias: Symbol::from(self.alias.as_str()),
            func: self.func,
            field: Symbol::from(self.field.as_str()),
            distinct: self.distinct,
        }
    }
}

/// `COUNT(field) AS alias`
pub fn count(alias: &str, field: &str) -> AggExpr {
    AggExpr {
        alias: alias.to_string(),
        func: AggFunc::Count,
        field: field.to_string(),
        distinct: false,
    }
}

/// `SUM(field) AS alias`
pub fn sum(alias: &str, field: &str) -> AggExpr {
    AggExpr {
        alias: alias.to_string(),
        func: AggFunc::Sum,
        field: field.to_string(),
        distinct: false,
    }
}

/// `AVG(field) AS alias`
pub fn avg(alias: &str, field: &str) -> AggExpr {
    AggExpr {
        alias: alias.to_string(),
        func: AggFunc::Avg,
        field: field.to_string(),
        distinct: false,
    }
}

/// `MIN(field) AS alias`
pub fn min(alias: &str, field: &str) -> AggExpr {
    AggExpr {
        alias: alias.to_string(),
        func: AggFunc::Min,
        field: field.to_string(),
        distinct: false,
    }
}

/// `MAX(field) AS alias`
pub fn max(alias: &str, field: &str) -> AggExpr {
    AggExpr {
        alias: alias.to_string(),
        func: AggFunc::Max,
        field: field.to_string(),
        distinct: false,
    }
}

/// `COUNT(DISTINCT field) AS alias`
pub fn count_distinct(alias: &str, field: &str) -> AggExpr {
    AggExpr {
        alias: alias.to_string(),
        func: AggFunc::Count,
        field: field.to_string(),
        distinct: true,
    }
}
