#![feature(proc_macro_hygiene)]
#![feature(decl_macro)]

#[macro_use] extern crate rocket;

use std::error::Error;
use std::fs::File;
use std::io::Read;

use clap::{App, Arg};
use url::Url;
use yaml_rust::{Yaml, YamlLoader};

use jsonvalue::JsonValue;
use postgres::{Connection, TlsMode};

mod jsonvalue;

// TODO restrict POST body size to prevent DoS attacks

#[post("/apps/<app_id>/<type_id>", format = "application/json", data = "<data>")]
fn post_event(app_id: String, type_id: String, data: JsonValue) {
}

fn read_yaml_file(file_name: &str) -> Result<Yaml, Box<Error>> {
    let mut contents = String::new();
    let mut file = File::open(file_name)?;
    file.read_to_string(&mut contents)?;
    let yaml = YamlLoader::load_from_str(&contents)?;
    Ok(yaml[0].clone())
}

fn connect_to_db(db_config: &Yaml) -> Result<Connection, postgres::Error> {
    let mut url = Url::parse("postgres://").unwrap();
    url.set_host(Some(db_config["host"].as_str().unwrap_or("localhost"))).unwrap();
    url.set_port(db_config["port"].as_i64().map(|port| port as u16)).unwrap();
    url.set_username(db_config["user"].as_str().unwrap_or("")).unwrap();
    url.set_password(db_config["password"].as_str()).unwrap();
    url.set_path(&("/".to_owned() + db_config["database"].as_str().unwrap_or(url.username())));
    Connection::connect(url.to_string(), TlsMode::None)
}

fn main() {
    let matches = App::new("Attolytics")
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
    let config = read_yaml_file(config_file_name)
        .unwrap_or_else(|err| panic!("failed to read config file {}: {}", config_file_name, err));

    let db = connect_to_db(&config["database"])
        .unwrap_or_else(|err| panic!("failed to open database: {}", err));

    rocket::ignite()
        .mount("/", routes![post_event])
        .launch();
}
