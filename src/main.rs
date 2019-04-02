#![feature(proc_macro_hygiene)]
#![feature(decl_macro)]

#[macro_use] extern crate rocket;

use std::error::Error;
use std::fs;
use std::process::exit;

use clap::Arg;
use r2d2::Pool;
use r2d2_postgres::{PostgresConnectionManager, TlsMode};
use rocket::http::Status;
use rocket::State;

use config::Config;
use jsonvalue::JsonValue;
use std::fmt::Display;

mod config;
mod db;
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
        db::insert_event(&table, &conn, &event)
            .map_err(|err| {
                println!("error inserting event into database: {}", err);
                Status::InternalServerError
            })?;
    }

    Ok("".to_owned())
}

#[derive(Debug)]
struct RunError(String);

impl Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        write!(f, "{}", self.0)
    }
}

impl Error for RunError {}

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
    let config_yaml_str = fs::read_to_string(config_file_name)
        .map_err(|err| RunError(format!("failed to read config file {}: {}", config_file_name, err)))?;
    let config = Config::from_yaml(&config_yaml_str)
        .map_err(|err| RunError(format!("failed to parse config file {}: {}", config_file_name, err)))?;

    let manager = PostgresConnectionManager::new(config.database_url.to_owned(), TlsMode::None)
        .map_err(|err| RunError(format!("failed to open database: {}", err)))?;
    let db_conn_pool = Pool::new(manager)
        .map_err(|err| RunError(format!("failed to create connection pool: {}", err)))?;

    let conn = db_conn_pool.get()
        .map_err(|err| RunError(format!("failed to create database connection: {}", err)))?;
    db::create_tables(&config, &conn)
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
