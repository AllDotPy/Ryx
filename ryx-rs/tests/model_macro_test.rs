use ryx_rs::model;
use ryx_rs::Model;

/// Verify that `#[model]` derives Serialize + Deserialize.
#[model]
#[table("test_items")]
struct TestItem {
    #[field(pk)]
    id: i64,
    name: String,
    value: f64,
}

#[test]
fn test_serde_roundtrip() {
    let item = TestItem {
        id: 42,
        name: "answer".into(),
        value: 3.14,
    };

    let json = serde_json::to_string(&item).unwrap();
    assert!(json.contains("\"id\":42"));
    assert!(json.contains("\"name\":\"answer\""));

    let _decoded: TestItem = serde_json::from_str(&json).unwrap();
}

#[test]
fn test_field_meta() {
    let meta = TestItem::field_meta();
    assert_eq!(meta.len(), 3);

    let id = &meta[0];
    assert_eq!(id.name, "id");
    assert_eq!(id.db_type, "BIGINT");
    assert!(id.primary_key);
    assert!(!id.nullable);

    let name = &meta[1];
    assert_eq!(name.name, "name");
    assert_eq!(name.db_type, "TEXT");
    assert!(!name.primary_key);

    let val = &meta[2];
    assert_eq!(val.name, "value");
    assert_eq!(val.db_type, "DOUBLE PRECISION");
}
