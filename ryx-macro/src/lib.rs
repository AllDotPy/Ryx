use proc_macro::TokenStream;
use quote::quote;
use syn::{
    parse_macro_input, Attribute, Data, DeriveInput, Fields, ItemStruct, Lit, Meta,
    MetaNameValue, Type,
};

/// Parsed content of a single `#[field(...)]` attribute.
struct FieldAttr {
    column: Option<String>,
    pk: bool,
    db_type: Option<String>,
    nullable: bool,
    unique: bool,
    default: Option<String>,
}

fn parse_field_attr(field: &syn::Field) -> FieldAttr {
    let mut result = FieldAttr {
        column: None,
        pk: false,
        db_type: None,
        nullable: false,
        unique: false,
        default: None,
    };
    for attr in &field.attrs {
        if !attr.path().is_ident("field") {
            continue;
        }
        if let Meta::List(list) = &attr.meta {
            let tokens = &list.tokens;
            let raw = quote!(#tokens).to_string();
            for segment in raw.split(',') {
                let segment = segment.trim();
                if segment == "pk" {
                    result.pk = true;
                } else if segment == "nullable" {
                    result.nullable = true;
                } else if segment == "unique" {
                    result.unique = true;
                } else if let Some(val) = segment.strip_prefix("column = ") {
                    result.column = Some(
                        val.trim()
                            .trim_matches('"')
                            .trim_matches('\'')
                            .to_string(),
                    );
                } else if let Some(val) = segment.strip_prefix("db_type = ") {
                    result.db_type = Some(
                        val.trim()
                            .trim_matches('"')
                            .trim_matches('\'')
                            .to_string(),
                    );
                } else if let Some(val) = segment.strip_prefix("default = ") {
                    result.default = Some(
                        val.trim()
                            .trim_matches('"')
                            .trim_matches('\'')
                            .to_string(),
                    );
                }
            }
        }
    }
    result
}

/// Parsed content of a `#[relation(...)]` struct-level attribute.
struct RelationAttr {
    name: String,
    fk_column: String,
    to_table: String,
    to_field: String,
}

fn parse_relation_attrs(attrs: &[Attribute]) -> Vec<RelationAttr> {
    let mut relations = Vec::new();
    for attr in attrs {
        if !attr.path().is_ident("relation") {
            continue;
        }
        if let Meta::List(list) = &attr.meta {
            let tokens = &list.tokens;
            let raw = quote!(#tokens).to_string();
            let mut rel = RelationAttr {
                name: String::new(),
                fk_column: String::new(),
                to_table: String::new(),
                to_field: "id".to_string(),
            };
            for segment in raw.split(',') {
                let segment = segment.trim();
                if let Some(val) = segment.strip_prefix("name = ") {
                    rel.name = val.trim().trim_matches('"').trim_matches('\'').to_string();
                } else if let Some(val) = segment.strip_prefix("fk_column = ") {
                    rel.fk_column = val.trim().trim_matches('"').trim_matches('\'').to_string();
                } else if let Some(val) = segment.strip_prefix("to_table = ") {
                    rel.to_table = val.trim().trim_matches('"').trim_matches('\'').to_string();
                } else if let Some(val) = segment.strip_prefix("to_field = ") {
                    rel.to_field = val.trim().trim_matches('"').trim_matches('\'').to_string();
                }
            }
            if !rel.name.is_empty() && !rel.fk_column.is_empty() && !rel.to_table.is_empty() {
                relations.push(rel);
            }
        }
    }
    relations
}

fn generate_relationships_impl(name: &syn::Ident, relations: &[RelationAttr]) -> proc_macro2::TokenStream {
    if relations.is_empty() {
        return quote! {};
    }
    let entries: Vec<_> = relations
        .iter()
        .map(|r| {
            let rel_name = &r.name;
            let fk_col = &r.fk_column;
            let to_tbl = &r.to_table;
            let to_fld = &r.to_field;
            quote! {
                ::ryx_rs::model::RelationMeta {
                    name: #rel_name,
                    fk_column: #fk_col,
                    to_table: #to_tbl,
                    to_field: #to_fld,
                }
            }
        })
        .collect();

    quote! {
        impl ::ryx_rs::model::Relationships for #name {
            fn relations() -> &'static [::ryx_rs::model::RelationMeta] {
                &[#(#entries),*]
            }
        }
    }
}

/// Map a Rust type to a SQL column type string.
fn rust_type_to_sql(ty: &Type, field_attr: &FieldAttr) -> String {
    // #[field(db_type = "...")] takes priority
    if let Some(ref custom) = field_attr.db_type {
        return custom.clone();
    }

    // Match on the outer type (peel Option first)
    let (inner_ty, _is_option) = peel_option(ty);
    let type_str = type_ident_str(inner_ty).unwrap_or_else(|| "?".to_string());

    match type_str.as_str() {
        "i32" | "Int32" => "INTEGER".to_string(),
        "i64" | "Int64" => "BIGINT".to_string(),
        "f32" => "REAL".to_string(),
        "f64" => "DOUBLE PRECISION".to_string(),
        "bool" | "Bool" => "BOOLEAN".to_string(),
        "String" => "TEXT".to_string(),
        "NaiveDateTime" | "DateTime" => "TIMESTAMP".to_string(),
        "NaiveDate" | "Date" => "DATE".to_string(),
        "NaiveTime" | "Time" => "TIME".to_string(),
        "Uuid" => "UUID".to_string(),
        "serde_json::Value" | "Value" => "JSONB".to_string(),
        _ => "TEXT".to_string(), // fallback
    }
}

/// Peel one level of Option<T>, return (inner_type, is_option).
fn peel_option(ty: &Type) -> (&Type, bool) {
    if let Type::Path(type_path) = ty {
        if let Some(first) = type_path.path.segments.first() {
            if first.ident == "Option" {
                if let syn::PathArguments::AngleBracketed(args) = &first.arguments {
                    if let Some(syn::GenericArgument::Type(inner)) = args.args.first() {
                        return (inner, true);
                    }
                }
            }
        }
    }
    (ty, false)
}

/// Get the last path segment identifier of a type as a string.
fn type_ident_str(ty: &Type) -> Option<String> {
    if let Type::Path(type_path) = ty {
        type_path
            .path
            .segments
            .last()
            .map(|s| s.ident.to_string())
    } else {
        None
    }
}

fn find_table_name(attrs: &[Attribute]) -> String {
    for attr in attrs {
        if attr.path().is_ident("table") {
            match &attr.meta {
                Meta::List(list) => {
                    if let Ok(lit) = syn::parse2::<syn::LitStr>(list.tokens.clone()) {
                        return lit.value();
                    }
                }
                Meta::NameValue(MetaNameValue {
                    value:
                        syn::Expr::Lit(syn::ExprLit {
                            lit: Lit::Str(s), ..
                        }),
                    ..
                }) => return s.value(),
                _ => {}
            }
        }
    }
    String::new()
}

fn field_column_name(field: &syn::Field) -> String {
    parse_field_attr(field)
        .column
        .unwrap_or_else(|| {
            field
                .ident
                .as_ref()
                .map(|i| i.to_string())
                .unwrap_or_default()
        })
}

fn is_pk_field(field: &syn::Field) -> bool {
    parse_field_attr(field).pk
}

fn type_to_sql_reader(ty: &Type, col_name: &str) -> proc_macro2::TokenStream {
    let col_str = col_name.to_string();

    match ty {
        Type::Path(type_path) => {
            let ident = &type_path.path.segments.last().unwrap().ident;
            let ident_str = ident.to_string();

            // Handle Option<T>
            if type_path.path.segments.first().unwrap().ident == "Option" {
                let inner_type = &type_path.path.segments.last().unwrap();
                if let syn::PathArguments::AngleBracketed(args) = &inner_type.arguments {
                    if let Some(syn::GenericArgument::Type(inner_ty)) = args.args.first() {
                        let inner = type_to_sql_reader(inner_ty, col_name);
                        return quote! {
                            match row.get(#col_str) {
                                Some(v) => Some({ #inner }),
                                None => None,
                            }
                        };
                    }
                }
                return quote! { row.get(#col_str).and_then(|v| v.as_null().map(|_| None)).unwrap_or(None) };
            }

            let sv = quote! { ::ryx_rs::SqlValue };
            match ident_str.as_str() {
                "i32" => quote! {
                    row.get(#col_str).and_then(|v: &#sv| match v {
                        #sv::Int(n) => Some(*n as i32),
                        _ => None,
                    }).ok_or_else(|| internal_err(#col_str, "i32"))?
                },
                "i64" => quote! {
                    row.get(#col_str).and_then(|v: &#sv| match v {
                        #sv::Int(n) => Some(*n),
                        _ => None,
                    }).ok_or_else(|| internal_err(#col_str, "i64"))?
                },
                "String" => quote! {
                    row.get(#col_str).and_then(|v: &#sv| match v {
                        #sv::Text(s) => Some(s.clone()),
                        _ => None,
                    }).ok_or_else(|| internal_err(#col_str, "String"))?
                },
                "bool" => quote! {
                    row.get(#col_str).and_then(|v: &#sv| match v {
                        #sv::Bool(b) => Some(*b),
                        #sv::Int(n) => Some(*n != 0),
                        _ => None,
                    }).ok_or_else(|| internal_err(#col_str, "bool"))?
                },
                "f64" => quote! {
                    row.get(#col_str).and_then(|v: &#sv| match v {
                        #sv::Float(f) => Some(*f),
                        #sv::Int(n) => Some(*n as f64),
                        _ => None,
                    }).ok_or_else(|| internal_err(#col_str, "f64"))?
                },
                _ => {
                    // Try chrono types
                    if ident_str == "NaiveDateTime" {
                        quote! {
                            row.get(#col_str).and_then(|v: &#sv| match v {
                                #sv::Text(s) => chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").ok(),
                                _ => None,
                            }).ok_or_else(|| internal_err(#col_str, "NaiveDateTime"))?
                        }
                    } else if ident_str == "NaiveDate" {
                        quote! {
                            row.get(#col_str).and_then(|v: &#sv| match v {
                                #sv::Text(s) => chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok(),
                                _ => None,
                            }).ok_or_else(|| internal_err(#col_str, "NaiveDate"))?
                        }
                    } else {
                        // Fallback: try to parse as string
                        quote! {
                            row.get(#col_str).and_then(|v: &#sv| match v {
                                #sv::Text(s) => Some(s.parse().ok()),
                                _ => None,
                            }).flatten().ok_or_else(|| internal_err(#col_str, #ident_str))?
                        }
                    }
                }
            }
        }
        _ => quote! { compile_error!("unsupported field type") },
    }
}

fn field_names(fields: &Fields) -> Vec<String> {
    let mut names = Vec::new();
    match fields {
        Fields::Named(named) => {
            for field in &named.named {
                let col = field_column_name(field);
                let name = if col.is_empty() {
                    field.ident.as_ref().map(|i| i.to_string()).unwrap_or_default()
                } else {
                    col
                };
                names.push(name);
            }
        }
        _ => {}
    }
    names
}

fn pk_field_name(fields: &Fields) -> String {
    match fields {
        Fields::Named(named) => {
            for field in &named.named {
                if is_pk_field(field) {
                    return field.ident.as_ref().map(|i| i.to_string()).unwrap_or_default();
                }
            }
        }
        _ => {}
    }
    "id".to_string()
}

#[proc_macro_derive(Model, attributes(table, field, relation))]
pub fn derive_model(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let relations = parse_relation_attrs(&input.attrs);
    let table_name = find_table_name(&input.attrs);
    let table_name = if table_name.is_empty() {
        // Convert PascalCase to snake_case
        let s = name.to_string();
        let mut result = String::with_capacity(s.len() + 5);
        for (i, c) in s.chars().enumerate() {
            if c.is_uppercase() && i > 0 {
                result.push('_');
                result.push(c.to_ascii_lowercase());
            } else {
                result.push(c.to_ascii_lowercase());
            }
        }
        result
    } else {
        table_name
    };

    let fields = match &input.data {
        Data::Struct(data) => &data.fields,
        _ => panic!("Model derive only supports structs"),
    };

    let fnames = field_names(fields);
    let pk_field = pk_field_name(fields);

    // Build FieldMeta entries (same logic as #[model])
    let field_meta_entries: Vec<_> = match fields {
        syn::Fields::Named(named) => named
            .named
            .iter()
            .map(|field| {
                let fattr = parse_field_attr(field);
                let col_name = fattr.column.clone().unwrap_or_else(|| {
                    field
                        .ident
                        .as_ref()
                        .map(|i| i.to_string())
                        .unwrap_or_default()
                });
                let db_type = rust_type_to_sql(&field.ty, &fattr);
                let nullable = fattr.nullable || peel_option(&field.ty).1;
                let pk = fattr.pk;
                let unique = fattr.unique;
                let default = match &fattr.default {
                    Some(d) => {
                        let lit = syn::LitStr::new(d, proc_macro2::Span::call_site());
                        quote! { Some(#lit) }
                    }
                    None => quote! { None },
                };
                quote! {
                    ::ryx_rs::model::FieldMeta {
                        name: #col_name,
                        db_type: #db_type,
                        nullable: #nullable,
                        primary_key: #pk,
                        unique: #unique,
                        default: #default,
                    }
                }
            })
            .collect(),
        _ => vec![],
    };

    let relationships_impl = generate_relationships_impl(name, &relations);

    let expanded = quote! {
        impl ::ryx_rs::model::Model for #name {
            fn table_name() -> &'static str {
                #table_name
            }

            fn field_names() -> &'static [&'static str] {
                &[#(#fnames),*]
            }

            fn pk_field() -> &'static str {
                #pk_field
            }

            fn field_meta() -> &'static [::ryx_rs::model::FieldMeta] {
                &[#(#field_meta_entries),*]
            }
        }

        #relationships_impl
    };

    expanded.into()
}

#[proc_macro_derive(FromRow, attributes(field))]
pub fn derive_from_row(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;

    let fields = match &input.data {
        Data::Struct(data) => &data.fields,
        _ => panic!("FromRow derive only supports structs"),
    };

    let field_reads: Vec<_> = match fields {
        Fields::Named(named) => named
            .named
            .iter()
            .map(|field| {
                let col_name = field_column_name(field);
                let col_name = if col_name.is_empty() {
                    field
                        .ident
                        .as_ref()
                        .map(|i| i.to_string())
                        .unwrap_or_default()
                } else {
                    col_name
                };
                let field_name = &field.ident;
                let reader = type_to_sql_reader(&field.ty, &col_name);
                quote! { #field_name: #reader }
            })
            .collect(),
        _ => vec![],
    };

    let expanded = quote! {
        impl FromRow for #name {
            fn from_row(row: &RowView) -> ::ryx_rs::RyxResult<Self> {
                fn internal_err(col: &str, ty: &str) -> ::ryx_rs::RyxError {
                    ::ryx_rs::RyxError::Internal(format!(
                        "Failed to decode column '{}' as {}", col, ty
                    ))
                }
                Ok(Self {
                    #(#field_reads),*
                })
            }
        }
    };

    expanded.into()
}

/// Attribute macro that marks a struct as a database model with automatic
/// `Serialize`/`Deserialize` derives.
///
/// Combines `Model`, `FromRow`, `Serialize`, and `Deserialize` in one step.
///
/// # Usage
///
/// ```ignore
/// #[model]
/// #[table("posts")]
/// struct Post {
///     #[field(pk)]
///     id: i64,
///     title: String,
///     #[field(column = "is_active")]
///     active: bool,
/// }
/// ```
///
/// The `#[table]` and `#[field]` attributes work the same as with
/// `#[derive(Model, FromRow)]`.
#[proc_macro_attribute]
pub fn model(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut strukt: ItemStruct = syn::parse(item).expect("expected a struct");
    let name = &strukt.ident;
    let fields = &strukt.fields;
    let relations = parse_relation_attrs(&strukt.attrs);

    let data_fields = match fields {
        syn::Fields::Named(named) => {
            let list = named.named.clone();
            syn::Fields::Named(syn::FieldsNamed {
                brace_token: named.brace_token,
                named: list,
            })
        }
        other => other.clone(),
    };

    let fnames = field_names(&data_fields);
    let pk = pk_field_name(&data_fields);

    // Build FieldMeta array entries
    let field_meta_entries: Vec<_> = match &data_fields {
        syn::Fields::Named(named) => named
            .named
            .iter()
            .map(|field| {
                let fattr = parse_field_attr(field);
                let col_name = fattr.column.clone().unwrap_or_else(|| {
                    field
                        .ident
                        .as_ref()
                        .map(|i| i.to_string())
                        .unwrap_or_default()
                });
                let db_type = rust_type_to_sql(&field.ty, &fattr);
                let nullable = fattr.nullable || peel_option(&field.ty).1;
                let pk = fattr.pk;
                let unique = fattr.unique;
                let default = match &fattr.default {
                    Some(d) => {
                        // Generate `Some("...")` with the string value as a literal
                        let lit = syn::LitStr::new(d, proc_macro2::Span::call_site());
                        quote! { Some(#lit) }
                    }
                    None => quote! { None },
                };
                quote! {
                    ::ryx_rs::model::FieldMeta {
                        name: #col_name,
                        db_type: #db_type,
                        nullable: #nullable,
                        primary_key: #pk,
                        unique: #unique,
                        default: #default,
                    }
                }
            })
            .collect(),
        _ => vec![],
    };

    // Compute table name: from #[table(...)] attribute or PascalCase→snake_case
    let table_name = find_table_name(&strukt.attrs);
    let table_name = if table_name.is_empty() {
        let s = name.to_string();
        let mut result = String::with_capacity(s.len() + 5);
        for (i, c) in s.chars().enumerate() {
            if c.is_uppercase() && i > 0 {
                result.push('_');
                result.push(c.to_ascii_lowercase());
            } else {
                result.push(c.to_ascii_lowercase());
            }
        }
        result
    } else {
        table_name
    };

    // Build FromRow field reads
    let field_reads: Vec<_> = match &data_fields {
        syn::Fields::Named(named) => named
            .named
            .iter()
            .map(|field| {
                let col_name = field_column_name(field);
                let col_name = if col_name.is_empty() {
                    field
                        .ident
                        .as_ref()
                        .map(|i| i.to_string())
                        .unwrap_or_default()
                } else {
                    col_name
                };
                let field_name = &field.ident;
                let reader = type_to_sql_reader(&field.ty, &col_name);
                quote! { #field_name: #reader }
            })
            .collect(),
        _ => vec![],
    };

    // Strip #[table], #[field], #[relation] helper attrs
    strukt.attrs.retain(|a| {
        !a.path().is_ident("table") && !a.path().is_ident("field") && !a.path().is_ident("relation")
    });
    // Also strip #[field] from each field
    if let syn::Fields::Named(ref mut named) = strukt.fields {
        for field in &mut named.named {
            field.attrs.retain(|a| !a.path().is_ident("field"));
        }
    }

    let relationships_impl2 = generate_relationships_impl(name, &relations);

    let expanded = quote! {
        #[derive(::ryx_rs::serde::Serialize, ::ryx_rs::serde::Deserialize)]
        #strukt

        impl ::ryx_rs::model::Model for #name {
            fn table_name() -> &'static str {
                #table_name
            }

            fn field_names() -> &'static [&'static str] {
                &[#(#fnames),*]
            }

            fn pk_field() -> &'static str {
                #pk
            }

            fn field_meta() -> &'static [::ryx_rs::model::FieldMeta] {
                &[#(#field_meta_entries),*]
            }
        }

        impl ::ryx_rs::row::FromRow for #name {
            fn from_row(row: &::ryx_rs::row::RowView) -> ::ryx_rs::RyxResult<Self> {
                fn internal_err(col: &str, ty: &str) -> ::ryx_rs::RyxError {
                    ::ryx_rs::RyxError::Internal(format!(
                        "Failed to decode column '{}' as {}", col, ty
                    ))
                }
                Ok(Self {
                    #(#field_reads),*
                })
            }
        }

        #relationships_impl2
    };

    expanded.into()
}
