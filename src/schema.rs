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
    pub tables: Vec<String>,
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
    #[serde(rename = "type")]
    pub type_: Type,
    #[serde(default)]
    pub indexed: bool,
    #[serde(default)]
    pub required: bool,
}

#[derive(Debug)]
pub enum SchemaError {
    YamlParseError(serde_yaml::Error),
    TableNotFound { app_id: String, table_name: String },
}

impl Display for SchemaError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        match self {
            SchemaError::YamlParseError(err) =>
                write!(f, "{}", err),
            SchemaError::TableNotFound {app_id, table_name} =>
                write!(f, "app {} refers to undefined table {}", app_id, table_name),
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
    let mut file = File::open("example.conf.yaml").unwrap();
    file.read_to_string(&mut contents).unwrap();
    let schema = Schema::from_yaml(&contents).unwrap();
    let expected_schema = Schema {
        database_url: "postgres://myuser:mypassword@localhost:5432/attolytics".to_string(),
        tables: [
            ("events".to_string(), Table {
                name: "events".to_string(),
                columns: vec![
                    Column {
                        name: "time".to_string(),
                        type_: Type::Timestamp,
                        indexed: true,
                        required: false,
                    },
                    Column {
                        name: "platform".to_string(),
                        type_: Type::String,
                        indexed: true,
                        required: true,
                    },
                    Column {
                        name: "version".to_string(),
                        type_: Type::String,
                        indexed: true,
                        required: true,
                    },
                    Column {
                        name: "user_id".to_string(),
                        type_: Type::String,
                        indexed: false,
                        required: false,
                    },
                    Column {
                        name: "event_type".to_string(),
                        type_: Type::String,
                        indexed: true,
                        required: true,
                    },
                    Column {
                        name: "score".to_string(),
                        type_: Type::I32,
                        indexed: false,
                        required: false,
                    }
                ],
            }),
        ].iter().cloned().collect(),
        apps: [
            ("com.example.myapp".to_string(), App {
                app_id: "com.example.myapp".to_string(),
                secret_key: "n6MrfBnXcB7pIEeKdiCBmT8AqLEmtfUO".to_string(),
                tables: vec!["events".to_string()],
            }),
        ].iter().cloned().collect(),
    };
    assert_eq!(schema, expected_schema);
}
