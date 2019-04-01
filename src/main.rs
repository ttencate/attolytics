#![feature(proc_macro_hygiene)]
#![feature(decl_macro)]

#[macro_use] extern crate rocket;

use std::error::Error;
use std::fs::File;
use std::io::Read;

use clap::{App, Arg};
use yaml_rust::{Yaml, YamlLoader};

#[post("/apps/<app_id>/<type_id>")]
fn post_event(app_id: String, type_id: String) {
}

fn read_yaml_file(file_name: &str) -> Result<Yaml, Box<Error>> {
    let mut contents = String::new();
    let mut file = File::open(file_name)?;
    file.read_to_string(&mut contents)?;
    let yaml = YamlLoader::load_from_str(&contents)?;
    Ok(yaml[0].clone())
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
            .default_value("attolytics.conf.yml"))
        .get_matches();

    let config = read_yaml_file(matches.value_of("config").unwrap())
        .expect("failed to read config file");

    rocket::ignite()
        .mount("/", routes![post_event])
        .launch();
}
