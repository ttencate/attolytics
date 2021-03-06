use std::collections::HashMap;
use std::error::Error;
use std::fmt::Display;
#[cfg(test)]
use std::fs::File;
#[cfg(test)]
use std::io::Read;

use serde::Deserialize;

use crate::types::Type;

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
pub struct Schema {
    pub tables: HashMap<String, Table>,
    pub apps: HashMap<String, App>,
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
pub struct App {
    #[serde(skip)]
    pub app_id: String,
    pub secret_key: String,
    #[serde(default = "default_access_control_allow_origin")]
    pub access_control_allow_origin: String,
    pub tables: Vec<String>,
}

fn default_access_control_allow_origin() -> String {
    "*".to_string()
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
pub struct Table {
    #[serde(skip)]
    pub name: String,
    pub columns: Vec<Column>,
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
pub struct Column {
    pub name: String,
    #[serde(rename = "type", default)]
    pub type_: Type,
    #[serde(default)]
    pub header: Option<String>,
    #[serde(default)]
    pub indexed: bool,
    #[serde(default)]
    pub required: bool,
}

#[derive(Debug)]
pub enum SchemaError {
    YamlParseError(serde_yaml::Error),
    TableNotFound { app_id: String, table_name: String },
    WrongColumnType { actual: Type, expected: Type },
}

impl Display for SchemaError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        match self {
            SchemaError::YamlParseError(err) =>
                write!(f, "{}", err),
            SchemaError::TableNotFound {app_id, table_name} =>
                write!(f, "app {} refers to undefined table {}", app_id, table_name),
            SchemaError::WrongColumnType {actual, expected} =>
                write!(f, "column type should be {:?} here, but was {:?}", expected, actual),
        }
    }
}

impl Error for SchemaError {}

impl Schema {
    pub fn from_yaml(yaml_str: &str) -> Result<Schema, SchemaError> {
        let mut schema = serde_yaml::from_str::<Schema>(yaml_str)
            .map_err(|err| SchemaError::YamlParseError(err))?;
        for (table_name, table) in &mut schema.tables {
            table.name = table_name.to_string();
            for column in &mut table.columns {
                if column.header.is_some() && column.type_ != Type::String {
                    return Err(SchemaError::WrongColumnType { actual: column.type_.clone(), expected: Type::String })
                }
            }
        }
        for (app_id, app) in &mut schema.apps {
            app.app_id = app_id.to_string();
            for table_name in &app.tables {
                if !schema.tables.contains_key(table_name) {
                    return Err(SchemaError::TableNotFound {app_id: app_id.to_string(), table_name: table_name.to_string()})
                }
            }
        }
        Ok(schema)
    }
}

#[test]
fn parse_example_schema() {
    let mut contents = String::new();
    let mut file = File::open("schema-example.conf.yaml").unwrap();
    file.read_to_string(&mut contents).unwrap();
    let schema = Schema::from_yaml(&contents).unwrap();
    let expected_schema = Schema {
        tables: [
            ("events".to_string(), Table {
                name: "events".to_string(),
                columns: vec![
                    Column {
                        name: "time".to_string(),
                        type_: Type::Timestamp,
                        header: None,
                        indexed: true,
                        required: false,
                    },
                    Column {
                        name: "referer".to_string(),
                        type_: Type::String,
                        header: Some("Referer".to_string()),
                        indexed: false,
                        required: false,
                    },
                    Column {
                        name: "platform".to_string(),
                        type_: Type::String,
                        header: None,
                        indexed: true,
                        required: true,
                    },
                    Column {
                        name: "version".to_string(),
                        type_: Type::String,
                        header: None,
                        indexed: true,
                        required: true,
                    },
                    Column {
                        name: "user_id".to_string(),
                        type_: Type::String,
                        header: None,
                        indexed: false,
                        required: false,
                    },
                    Column {
                        name: "event_type".to_string(),
                        type_: Type::String,
                        header: None,
                        indexed: true,
                        required: true,
                    },
                    Column {
                        name: "score".to_string(),
                        type_: Type::I32,
                        header: None,
                        indexed: false,
                        required: false,
                    }
                ],
            }),
        ].iter().cloned().collect(),
        apps: [
            ("com.example.myapp".to_string(), App {
                app_id: "com.example.myapp".to_string(),
                secret_key: "qD3eRda0709mD/3kGp4DlJtEQy5aMY0m".to_string(),
                access_control_allow_origin: "http://example.com".to_string(),
                tables: vec!["events".to_string()],
            }),
        ].iter().cloned().collect(),
    };
    assert_eq!(schema, expected_schema);
}
