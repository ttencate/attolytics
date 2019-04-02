#![feature(proc_macro_hygiene)]
#![feature(decl_macro)]

#[macro_use] extern crate rocket;

use std::collections::HashSet;
use std::error::Error;
use std::fs::File;
use std::io::Read;
use std::process::exit;

use clap::Arg;
use itertools::{Itertools, process_results};
use postgres::Connection;
use postgres::types::ToSql;
use r2d2::Pool;
use r2d2_postgres::{PostgresConnectionManager, TlsMode};
use rocket::http::Status;
use rocket::State;

use config::{Config, Table};
use jsonvalue::JsonValue;

mod config;
mod jsonvalue;
mod types;

// TODO restrict POST body size to prevent DoS attacks

#[post("/apps/<app_id>/events", format = "application/json", data = "<data>")]
fn post_event(app_id: String, data: JsonValue, config: State<Config>, db_conn_pool: State<Pool<PostgresConnectionManager>>) -> Result<String, Status> {
    let data = data.into_inner();

    let app = config.apps.get(&app_id)
        .ok_or(Status::NotFound)?;
    if data["secret_key"] != app.secret_key {
        return Err(Status::Forbidden);
    }

    // TODO don't insert anything until we've verified the entire request (use db transaction?)

    let conn = db_conn_pool.get()
        .map_err(|err| {
            println!("error connecting to database: {}", err);
            Status::InternalServerError
        })?;

    for event in data["events"].as_array().ok_or_else(|| Status::BadRequest)? {
        let table_name = event["_t"].as_str()
            .ok_or(Status::BadRequest)?
            .to_owned();
        if !app.tables.contains(&table_name) {
            return Err(Status::NotFound);
        }
        let table = config.tables.get(&table_name)
            .ok_or(Status::NotFound)?;
        insert_event(&table, &conn, &event)
            .map_err(|err| {
                println!("error inserting event into database: {}", err);
                Status::InternalServerError
            })?;
    }

    Ok("".to_owned())
}

fn insert_event(table: &Table, conn: &Connection, json: &serde_json::Value) -> Result<(), Box<Error>> {
    let query = format!(r#"INSERT INTO "{}" ({}) VALUES ({})"#,
        table.name,
        table.columns.iter().map(|column| format!(r#""{}""#, column.name)).join(", "),
        (1..=table.columns.len()).map(|idx| format!("${}", idx)).join(", "));
    let values: Vec<Box<ToSql>> = process_results(
        table.columns.iter()
            .map(|column| column.type_.json_to_sql(&column.name, &json[&column.name], column.required)),
        |iter| iter.collect())?;
    println!("{} {:?}", query, values);
    conn.execute(&query, &values.iter().map(|v| v.as_ref()).collect::<Vec<&ToSql>>())?;
    Ok(())
}

fn read_file(file_name: &str) -> Result<String, std::io::Error> {
    let mut contents = String::new();
    let mut file = File::open(file_name)?;
    file.read_to_string(&mut contents)?;
    Ok(contents)
}

fn create_tables(config: &Config, conn: &Connection) -> Result<(), postgres::Error> {
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

struct RunError(String);

fn run() -> Result<(), RunError> {
    let matches = clap::App::new("Attolytics")
        .about("A simple web server that stores analytics events into a database")
        .arg(Arg::with_name("config")
            .long("--config")
            .short("-c")
            .value_name("path/to/attolytics.conf.yaml")
            .help("Configuration file to use")
            .takes_value(true)
            .default_value("./attolytics.conf.yaml"))
        .get_matches();

    let config_file_name = matches.value_of("config").unwrap();
    let config_yaml_str = read_file(config_file_name)
        .map_err(|err| RunError(format!("failed to read config file {}: {}", config_file_name, err)))?;
    let config = Config::from_yaml(&config_yaml_str)
        .map_err(|err| RunError(format!("failed to parse config file {}: {}", config_file_name, err)))?;

    let manager = PostgresConnectionManager::new(config.database_url.to_owned(), TlsMode::None)
        .map_err(|err| RunError(format!("failed to open database: {}", err)))?;
    let db_conn_pool = Pool::new(manager)
        .map_err(|err| RunError(format!("failed to create connection pool: {}", err)))?;

    let conn = db_conn_pool.get()
        .map_err(|err| RunError(format!("failed to create database connection: {}", err)))?;
    create_tables(&config, &conn)
        .map_err(|err| RunError(format!("failed to create database tables: {}", err)))?;

    let err = rocket::ignite()
        .manage(config)
        .manage(db_conn_pool)
        .mount("/", routes![post_event])
        .launch();
    Err(RunError(format!("failed to launch web server: {}", err)))
}

fn main() {
    if let Err(RunError(msg)) = run() {
        eprintln!("error: {}", msg);
        exit(1);
    } else {
        exit(0);
    }
}
