#![feature(proc_macro_hygiene)]
#![feature(decl_macro)]

#[macro_use] extern crate rocket;

use std::error::Error;
use std::fmt::Display;
use std::fs;
use std::ops::Deref;
use std::process::exit;

use clap::{AppSettings, Arg};
use r2d2::Pool;
use r2d2_postgres::{PostgresConnectionManager, TlsMode};
use rocket::{Config, State};
use rocket::config::{Environment, Limits, LoggingLevel};
use rocket::fairing;
use rocket::http::{Status, HeaderMap};
use rocket::http::hyper::header::AccessControlAllowOrigin;
use rocket::outcome::Outcome;
use rocket::request::{FromRequest, Request};
use rocket_contrib::json::Json;
use serde::Deserialize;

use schema::Schema;
use db::DbError;

mod schema;
mod db;
mod types;

#[derive(Debug, Deserialize)]
struct EventPostData {
    secret_key: String,
    events: Vec<serde_json::Value>,
}

#[derive(Debug, Responder)]
struct CorsHeader {
    inner: String,
    header: AccessControlAllowOrigin,
}

#[derive(Debug)]
struct Headers<'a, 'r>(&'a HeaderMap<'r>);

impl<'a, 'r> FromRequest<'a, 'r> for Headers<'a, 'r> {
    type Error = ();
    fn from_request(request: &'a Request<'r>) -> rocket::request::Outcome<Self, Self::Error> {
        Outcome::Success(Headers(request.headers()))
    }
}

impl<'a, 'r> Deref for Headers<'a, 'r> {
    type Target = &'a HeaderMap<'r>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[options("/apps/<app_id>/events")]
fn options_events(app_id: String, schema: State<Schema>) -> Result<CorsHeader, Status> {
    let app = schema.apps.get(&app_id)
        .ok_or(Status::NotFound)?;
    Ok(CorsHeader {
        inner: "".to_string(),
        header: AccessControlAllowOrigin::Value(app.access_control_allow_origin.clone())
    })
}

#[post("/apps/<app_id>/events", format = "json", data = "<data>")]
fn post_events(
    app_id: String,
    headers: Headers,
    data: Json<EventPostData>,
    schema: State<Schema>,
    db_conn_pool: State<Pool<PostgresConnectionManager>>)
    -> Result<String, Status>
{
    let app = schema.apps.get(&app_id)
        .ok_or(Status::NotFound)?;
    if data.secret_key != app.secret_key {
        return Err(Status::Forbidden);
    }

    for event in &data.events {
        let table_name = event["_t"].as_str()
            .ok_or(Status::BadRequest)?
            .to_owned();
        if !app.tables.contains(&table_name) {
            return Err(Status::NotFound);
        }
    }

    let conn = db_conn_pool.get()
        .map_err(|err| {
            println!("error connecting to database: {}", err);
            Status::InternalServerError
        })?;
    let trans = conn.transaction()
        .map_err(|err| {
            println!("error starting transaction: {}", err);
            Status::InternalServerError
        })?;

    for event in &data.events {
        let table_name = event["_t"].as_str().unwrap();
        let table = schema.tables.get(table_name)
            .ok_or(Status::InternalServerError)?; // Table is in app.tables so it must be here.
        db::insert_event(&table, &trans, &event, &*headers)
            .map_err(|err| {
                println!("error inserting event into database: {}", err);
                match err {
                    DbError::ConversionError(_, _) => Status::BadRequest,
                    _ => Status::InternalServerError
                }
            })?;
    }

    trans.commit()
        .map_err(|err| {
            println!("error committing transaction: {}", err);
            Status::InternalServerError
        })?;

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

struct SystemdLaunchNotification {}

impl fairing::Fairing for SystemdLaunchNotification {
    fn info(&self) -> fairing::Info {
        fairing::Info { name: "systemd launch notifier", kind: fairing::Kind::Launch }
    }

    // "A launch callback, represented by the Fairing::on_launch() method, is called immediately
    // before the Rocket application has launched. At this point, Rocket has opened a socket for
    // listening but has not yet begun accepting connections."
    // It would be better if we could wait for the latter too, but there seems to be no support for
    // that in Rocket.
    fn on_launch(&self, _rocket: &rocket::Rocket) {
        match systemd::daemon::notify(true /* unset_environment */, [(systemd::daemon::STATE_READY, "1")].iter()) {
            Ok(true) => {},
            Ok(false) => eprintln!("failed to contact systemd"),
            Err(err) => eprintln!("failed to notify systemd of launch: {}", err),
        }
    }
}

fn run() -> Result<(), RunError> {
    let matches = clap::App::new("Attolytics")
        .author(clap::crate_authors!())
        .version(clap::crate_version!())
        .about("A simple web server that stores analytics events into a database")
        .setting(AppSettings::NextLineHelp)
        .arg(Arg::with_name("schema_file")
            .long("--schema").short("-s").value_name("path/to/schema.conf.yaml")
            .help("Schema configuration file to use")
            .takes_value(true).default_value("./schema.conf.yaml"))
        .arg(Arg::with_name("db_url")
             .long("--db_url").short("-d").value_name("postgres://user:pass@host:port/database")
             .help("URL of the PostgreSQL database; see https://github.com/sfackler/rust-postgres#connecting for the format")
             .takes_value(true).required(true))
        .arg(Arg::with_name("host")
             .long("--host").short("-H").value_name("host")
             .help("Hostname or IP address to listen on")
             .takes_value(true).default_value("localhost"))
        .arg(Arg::with_name("port")
             .long("--port").short("-p").value_name("port_number")
             .help("Port number to listen on")
             .takes_value(true).default_value("8000")
             .validator(|arg| arg.parse::<u16>().map(|_| ()).map_err(|err| format!("{}", err))))
        .arg(Arg::with_name("verbose")
             .long("--verbose").short("-v")
             .help("Produce more verbose logging; may be given up to 2 times")
             .multiple(true))
        .arg(Arg::with_name("quiet")
             .long("--quiet").short("-q")
             .help("Produce no output")
             .multiple(true))
        .get_matches();

    let schema_file_name = matches.value_of("schema_file").unwrap();
    let schema_yaml_str = fs::read_to_string(schema_file_name)
        .map_err(|err| RunError(format!("failed to read schema file {}: {}", schema_file_name, err)))?;
    let schema = Schema::from_yaml(&schema_yaml_str)
        .map_err(|err| RunError(format!("failed to parse schema file {}: {}", schema_file_name, err)))?;

    let manager = PostgresConnectionManager::new(matches.value_of("db_url").unwrap().to_owned(), TlsMode::None)
        .map_err(|err| RunError(format!("failed to open database: {}", err)))?;
    let db_conn_pool = Pool::new(manager)
        .map_err(|err| RunError(format!("failed to create connection pool: {}", err)))?;

    let conn = db_conn_pool.get()
        .map_err(|err| RunError(format!("failed to create database connection: {}", err)))?;
    db::create_tables(&schema, &*conn)
        .map_err(|err| RunError(format!("failed to initialize database tables: {}", err)))?;

    let verbosity = 1i32 + matches.occurrences_of("verbose") as i32 - matches.occurrences_of("quiet") as i32;
    let logging_level = match verbosity {
        0 => LoggingLevel::Off,
        1 => LoggingLevel::Critical,
        2 => LoggingLevel::Normal,
        3 => LoggingLevel::Debug,
        _ => if verbosity < 0 { LoggingLevel::Off } else { LoggingLevel::Debug },
    };
    let config = Config::build(Environment::active().map_err(|err| RunError(format!("invalid ROCKET_ENV value: {}", err)))?)
        .address(matches.value_of("host").unwrap())
        .port(matches.value_of("port").unwrap().parse::<u16>().unwrap())
        .keep_alive(0)
        .log_level(logging_level)
        .limits(Limits::new().limit("json", 32 * 1024))
        .finalize()
        .map_err(|err| RunError(format!("failed to create Rocket configuration: {}", err)))?;

    let err = rocket::custom(config)
        .manage(schema)
        .manage(db_conn_pool)
        .mount("/", routes![
            options_events,
            post_events,
        ])
        .attach(SystemdLaunchNotification {})
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
