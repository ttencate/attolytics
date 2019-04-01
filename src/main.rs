#![feature(proc_macro_hygiene)]
#![feature(decl_macro)]

#[macro_use] extern crate rocket;

use std::collections::HashMap;
use std::error::Error;
use std::fs::File;
use std::io::Read;

use clap::Arg;
use itertools::Itertools;
use postgres::types::ToSql;
use r2d2::Pool;
use r2d2_postgres::{PostgresConnectionManager, TlsMode};
use rocket::http::Status;
use rocket::State;
use url::Url;
use yaml_rust::{Yaml, YamlLoader};

use jsonvalue::JsonValue;
use postgres::Connection;

mod jsonvalue;

// TODO restrict POST body size to prevent DoS attacks

#[post("/apps/<app_id>/<type_id>", format = "application/json", data = "<data>")]
fn post_event(app_id: String, type_id: String, data: JsonValue, config: State<Config>, db_conn_pool: State<Pool<PostgresConnectionManager>>) -> Result<String, Status> {
    let data = data.into_inner();

    let app = config.apps.get(&app_id)
        .ok_or(Status::NotFound)?;
    if data["secret_key"] != app.secret_key {
        return Err(Status::Forbidden);
    }

    let event_type = app.event_types.get(&type_id)
        .ok_or(Status::NotFound)?;

    let conn = db_conn_pool.get()
        .map_err(|err| {
            println!("error connecting to database: {}", err);
            Status::InternalServerError
        })?;
    event_type.insert(&conn, &data)
        .map_err(|err| {
            println!("error inserting event into database: {}", err);
            Status::InternalServerError
        })?;

    Ok("".to_owned())
}

fn read_yaml_file(file_name: &str) -> Result<Yaml, Box<Error>> {
    let mut contents = String::new();
    let mut file = File::open(file_name)?;
    file.read_to_string(&mut contents)?;
    let yaml = YamlLoader::load_from_str(&contents)?;
    Ok(yaml[0].clone())
}

fn db_url(db_config: &Yaml) -> String {
    let mut url = Url::parse("postgres://").unwrap();
    url.set_host(Some(db_config["host"].as_str().unwrap_or("localhost"))).unwrap();
    url.set_port(db_config["port"].as_i64().map(|port| port as u16)).unwrap();
    url.set_username(db_config["user"].as_str().unwrap_or("")).unwrap();
    url.set_password(db_config["password"].as_str()).unwrap();
    url.set_path(&("/".to_owned() + db_config["database"].as_str().unwrap_or(url.username())));
    url.to_string()
}

#[derive(Debug)]
struct Config {
    apps: HashMap<String, App>
}

#[derive(Debug)]
struct App {
    app_id: String,
    secret_key: String,
    event_types: HashMap<String, EventType>,
}

#[derive(Debug)]
struct EventType {
    table_name: String,
    column_names: Vec<String>,
    query: String,
}

impl EventType {
    pub fn new(conn: &Connection, table_name: &str) -> Result<EventType, Box<Error>> {
        //https://stackoverflow.com/questions/20194806/how-to-get-a-list-column-names-and-datatype-of-a-table-in-postgresql
        let rows = conn.query(r#"
            SELECT
                a.attname as "column_name"
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
            "#,
            &[&table_name])?;
        let column_names: Vec<String> = rows.iter().map(|row| row.get(0)).collect();
        let query = format!(r#"INSERT INTO "{}" ({}) VALUES ({})"#,
            table_name,
            column_names.iter().map(|column| format!(r#""{}""#, column)).join(", "),
            (1..=column_names.len()).map(|idx| format!("${}", idx)).join(", "));
        Ok(EventType {
            table_name: String::from(table_name),
            column_names,
            query,
        })
    }

    pub fn insert(&self, conn: &Connection, json: &serde_json::Value) -> Result<(), Box<Error>> {
        let values: Vec<Box<ToSql>> = self.column_names.iter()
            .map(|column| json_to_sql(&json[column]))
            .collect();
        let value_refs: Vec<&ToSql> = values.iter()
            .map(|v| v.as_ref())
            .collect();
        conn.execute(&self.query, &value_refs)?;
        Ok(())
    }
}

fn json_to_sql(json: &serde_json::Value) -> Box<ToSql> {
    use serde_json::Value::*;
    match json {
        Null => Box::new(None as Option<bool>),
        Bool(bool) => Box::new(bool.clone()),
        Number(number) => match number {
            // There is no ToSql implementation for u64.
            _ if number.is_i64() || number.is_u64() => Box::new(number.as_i64()),
            _ => Box::new(number.as_f64()),
        }
        String(string) => Box::new(string.clone()),
        // Not supported, ignored.
        Array(_) => Box::new(None as Option<bool>),
        // Not supported, ignored.
        Object(_) => Box::new(None as Option<bool>),
    }
}

fn main() {
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
    let config_yaml = read_yaml_file(config_file_name)
        .unwrap_or_else(|err| panic!("failed to read config file {}: {}", config_file_name, err));

    let manager = PostgresConnectionManager::new(db_url(&config_yaml["database"]), TlsMode::None)
        .unwrap_or_else(|err| panic!("failed to open database: {}", err));
    let db_conn_pool = Pool::new(manager)
        .unwrap_or_else(|err| panic!("failed to create connection pool: {}", err));
    let conn = db_conn_pool.get()
        .unwrap_or_else(|err| panic!("failed to create database connection: {}", err));

    let mut apps: HashMap<String, App> = HashMap::new();
    for app in config_yaml["apps"].as_vec().unwrap_or(&vec![]) {
        let app_id = app["app_id"].as_str()
            .unwrap_or_else(|| panic!("no app_id specified for app"));
        let secret_key = app["secret_key"].as_str()
            .unwrap_or_else(|| panic!("no secret_key specified for app {}", app_id));
        let mut event_types = HashMap::new();
        for event in app["event_types"].as_vec().unwrap_or(&vec![]) {
            let type_id = event["type_id"].as_str()
                .unwrap_or_else(|| panic!("event type with no type_id specified for app {}", app_id));
            let event_type = EventType::new(&conn, type_id)
                .unwrap_or_else(|err| panic!("error creating event type {}: {}", type_id, err));
            event_types.insert(type_id.to_string(), event_type);
        }
        apps.insert(app_id.to_string(), App {
            app_id: app_id.to_string(),
            secret_key: secret_key.to_string(),
            event_types,
        });
    }
    let config = Config {
        apps
    };

    rocket::ignite()
        .manage(config)
        .manage(db_conn_pool)
        .mount("/", routes![post_event])
        .launch();
}
