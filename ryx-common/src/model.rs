#[derive(Clone, Debug)]
pub struct FieldMeta {
    pub name: String,
    pub column: String,
    pub primary_key: bool,
    pub data_type: String,
    pub nullable: bool,
    pub unique: bool,
}

#[derive(Clone, Debug)]
pub struct ModelMeta {
    pub name: String,
    pub table: String,
    pub app_label: Option<String>,
    pub database: Option<String>,
    pub ordering: Vec<String>,
    pub managed: bool,
    pub abstract_model: bool,
    pub fields: Vec<FieldMeta>,
}

impl ModelMeta {
    pub fn field(&self, name: &str) -> Option<&FieldMeta> {
        self.fields
            .iter()
            .find(|f| f.name == name || f.column == name)
    }
}
