//! Integration test: `StoredFile` inside a `#[model]` (macro + storage).

use ryx_rs::model;
use ryx_rs::storage::{InMemoryStorage, StoredFile};
use ryx_rs::into_sql::IntoSqlValue;
use ryx_rs::{Model, SqlValue};

#[model]
#[table("stored_items")]
struct StoredItem {
    #[field(pk)]
    id: i64,
    title: String,
    file: StoredFile,
}

#[test]
fn test_stored_file_field_meta_maps_to_varchar() {
    let metas = StoredItem::field_meta();
    let file_meta = metas.iter().find(|m| m.name == "file").unwrap();
    assert_eq!(file_meta.db_type, "VARCHAR(255)");
    let title_meta = metas.iter().find(|m| m.name == "title").unwrap();
    assert_eq!(title_meta.db_type, "TEXT");
}

#[test]
fn test_stored_file_into_sql_value() {
    let sf = StoredFile::new("docs/a.txt");
    match sf.into_sql_value() {
        SqlValue::Text(s) => assert_eq!(s, "docs/a.txt"),
        other => panic!("expected SqlValue::Text, got {:?}", other),
    }
}

#[tokio::test]
async fn test_stored_file_through_storage() {
    let storage = InMemoryStorage::new();
    let sf = StoredFile::save(&storage, "media/x.bin", b"BIN").await.unwrap();
    assert_eq!(sf.name(), "media/x.bin");
    assert_eq!(sf.open(&storage).await.unwrap(), b"BIN");
    assert_eq!(sf.url(&storage), "/media/media/x.bin");
    sf.delete(&storage).await.unwrap();
    assert!(!sf.exists(&storage).await.unwrap());
}