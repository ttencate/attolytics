use std::collections::HashSet;

use itertools::Itertools;
use postgres::GenericConnection;
use postgres::types::ToSql;
use crate::config::{Config, Table};
use std::fmt::Display;
use std::error::Error;
use crate::types::ConversionError;

#[derive(Debug)]
pub enum DbError {
    PostgresError(postgres::Error),
    ConversionError(String, ConversionError),
    StructureError(String),
}

impl Display for DbError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        match self {
            DbError::PostgresError(err) => write!(f, "{}", err),
            DbError::ConversionError(field, err) => write!(f, "error converting field \"{}\": {}", field, err),
            DbError::StructureError(msg) => write!(f, "{}", msg),
        }
    }
}

impl Error for DbError {}

impl From<postgres::Error> for DbError {
    fn from(err: postgres::Error) -> DbError {
        DbError::PostgresError(err)
    }
}

pub fn insert_event(table: &Table, conn: &GenericConnection, json: &serde_json::Value) -> Result<(), DbError> {
    let query = format!(r#"INSERT INTO "{}" ({}) VALUES ({})"#,
                        table.name,
                        table.columns.iter().map(|column| format!(r#""{}""#, column.name)).join(", "),
                        (1..=table.columns.len()).map(|idx| format!("${}", idx)).join(", "));
    let mut values = Vec::<Box<ToSql>>::with_capacity(table.columns.len());
    for column in &table.columns {
        let value = column.type_.json_to_sql(&column.name, &json[&column.name], column.required)
            .map_err(|err| DbError::ConversionError(column.name.to_string(), err))?;
        values.push(value);
    }
    // println!("{} {:?}", query, values);
    conn.execute(&query, &values.iter().map(|v| v.as_ref()).collect::<Vec<&ToSql>>())?;
    Ok(())
}

pub fn create_tables(config: &Config, conn: &GenericConnection) -> Result<(), DbError> {
    let existing_tables = conn.query(r#"
        SELECT relname
        FROM pg_catalog.pg_class
        WHERE pg_catalog.pg_table_is_visible(oid)
        "#, &[])?
        .iter()
        .map(|row| row.get(0))
        .collect::<HashSet<String>>();

    for table in config.tables.values() {
        if !existing_tables.contains(&table.name) {
            conn.execute(&creation_query(table), &[])?;
        } else {
            check_table(&table, conn)?;
        }
    }
    Ok(())
}

fn creation_query(table: &Table) -> String {
    let columns = table.columns
        .iter()
        .map(|column| format!(
            r#"{} {}{}"#,
            column.name,
            column.type_.postgres_type_name(),
            if column.required { " not null" } else { "" }
        ))
        .join(", ");
    format!(r#"
        CREATE TABLE "{}" ({})
        "#, table.name, columns)
}

fn check_table(table: &Table, conn: &GenericConnection) -> Result<(), DbError> {
    // https://stackoverflow.com/questions/20194806/how-to-get-a-list-column-names-and-datatype-of-a-table-in-postgresql
    let existing_columns = conn.query(r#"
        SELECT
            a.attname as "name",
            a.atttypid as "type_oid",
            pg_catalog.format_type(a.atttypid, a.atttypmod) as "postgres_type",
            a.attnotnull and not a.atthasdef as "required"
        FROM
            pg_catalog.pg_attribute a
        WHERE
            a.attnum > 0
            AND NOT a.attisdropped
            AND a.attrelid = (
                SELECT c.oid
                FROM pg_catalog.pg_class c
                    LEFT JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
                WHERE c.relname = $1
                    AND pg_catalog.pg_table_is_visible(c.oid)
            )
        "#, &[&table.name])?;
    for existing_column in &existing_columns {
        let name: String = existing_column.get("name");
        let type_oid: postgres::types::Oid = existing_column.get("type_oid");
        let postgres_type: String = existing_column.get("postgres_type");
        let required: bool = existing_column.get("required");

        let column = table.columns.iter().find(|column| column.name == name);
        match column {
            Some(column) => {
                if type_oid != column.type_.postgres_type().oid() {
                    return Err(DbError::StructureError(format!(
                        "table \"{}\" has column \"{}\" of type \"{}\", which does not match configured type \"{}\"",
                        table.name, name, postgres_type, column.type_.postgres_type_name())))
                }
                if required && !column.required {
                    return Err(DbError::StructureError(format!(
                        "table \"{}\" has non-nullable column \"{}\" which is not required in the configuration",
                        table.name, name)))
                }
            }
            None => {
                if required {
                    return Err(DbError::StructureError(format!(
                        "table \"{}\" has an extra required column \"{}\" that is not in the configuration",
                        table.name, name)).into())
                }
            }
        }
    }
    for column in &table.columns {
        let matching_column = existing_columns.iter().find(|c| c.get::<&str, String>("name") == column.name);
        if matching_column.is_none() {
            return Err(DbError::StructureError(format!(
                "table \"{}\" is missing configured column \"{}\"",
                table.name, column.name)));
        }
    }
    Ok(())
}
