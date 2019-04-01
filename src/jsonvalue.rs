// rocket_contrib contains a FromDataSimple implementation for typed Json<T> values, but not for
// untyped serde_json::Value. This file is a copy that rectifies the omission.

use std::io::Read;
use std::ops::{Deref, DerefMut};

use rocket::{Data, Request, response, Outcome};
use rocket::data::FromDataSimple;
use rocket::http::Status;
use rocket::response::{content, Responder};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::io;

/// An error returned by the [`Json`] data guard when incoming data fails to
/// serialize as JSON.
#[derive(Debug)]
pub enum JsonError {
    /// An I/O error occurred while reading the incoming request data.
    Io(io::Error),

    /// The client's data was received successfully but failed to parse as valid
    /// JSON.
    Parse(serde_json::error::Error),
}

impl From<io::Error> for JsonError {
    fn from(err: io::Error) -> Self {
        JsonError::Io(err)
    }
}

impl From<serde_json::error::Error> for JsonError {
    fn from(err: serde_json::error::Error) -> Self {
        JsonError::Parse(err)
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct JsonValue(pub serde_json::Value);

impl Serialize for JsonValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for JsonValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        serde_json::Value::deserialize(deserializer).map(JsonValue)
    }
}

impl JsonValue {
    #[inline(always)]
    fn into_inner(self) -> serde_json::Value {
        self.0
    }
}

impl Deref for JsonValue {
    type Target = serde_json::Value;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for JsonValue {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Into<serde_json::Value> for JsonValue {
    #[inline(always)]
    fn into(self) -> serde_json::Value {
        self.into_inner()
    }
}

impl From<serde_json::Value> for JsonValue {
    #[inline(always)]
    fn from(value: serde_json::Value) -> JsonValue {
        JsonValue(value)
    }
}

/// Serializes the value into JSON. Returns a response with Content-Type JSON
/// and a fixed-size body with the serialized value.
impl<'a> Responder<'a> for JsonValue {
    #[inline]
    fn respond_to(self, req: &Request) -> response::Result<'a> {
        content::Json(self.0.to_string()).respond_to(req)
    }
}

impl FromDataSimple for JsonValue {
    type Error = JsonError;

    fn from_data(request: &Request, data: Data) -> rocket::data::Outcome<Self, Self::Error> {
        let mut string = String::new();
        if let Err(err) = data.open().read_to_string(&mut string) {
            return Outcome::Failure((Status::InternalServerError, err.into()));
        }

        match serde_json::from_str(&string) {
            Ok(json) => Outcome::Success(json),
            Err(err) => Outcome::Failure((Status::BadRequest, err.into()))
        }
    }
}