Attolytics
==========

Attolytics (a portmanteau of the SI prefix "atto" meaning 10<sup>-18</sup> and
"analytics") is a small web service that receives analytics events and inserts
them into a PostgreSQL database. These events can subsequently be processed and
displayed using frameworks like [Cube.js](https://cube.dev/), but that is
outside the scope of this application.

Attolytics is written in [Rust](https://rust-lang.org/) using the
[Rocket](https://rocket.rs/) framework.

Compiling
---------

* Install Rust nightly, e.g.
  [using rustup](https://www.rust-lang.org/tools/install).

* Clone this repository:

        $ git clone https://github.com/ttencate/attolytics

* Compile the binary:

        $ cargo build

Running
-------

* Set up PostgreSQL using some appropriate guide for your system.

* Create a database, e.g. owned by your current user and named `attolytics`:

        $ createdb -o $(whoami) attolytics

* Create a table to contain your analytics events. Which columns it contains is
  up to you! For example:

        $ psql attolytics
        attolytics=> create table events (timestamp bigint, event_type varchar not null);

* Create a configuration file, typically named `attolytics.conf.yaml`. See
  `example.conf.yaml` for a documented example of the format.

* Run the executable, passing it the location of your configuration file:

        $ ./target/debug/attolytics --config ./attolytics.conf.yaml
