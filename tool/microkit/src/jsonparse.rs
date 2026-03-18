//
// Copyright 2025, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//

use crate::sel4::PlatformConfig;

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub struct JsonDoc {
    pub description: &'static str,
    pub value: serde_json::Value,
}

pub enum JsonError {
    ReadError {
        description: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    ParseError {
        description: &'static str,
        path: PathBuf,
        source: serde_json::Error,
    },
    TypeError {
        description: &'static str,
        field: &'static str,
        expected_type: &'static str,
    },
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReadError { description, path, source } => {
                write!(f, "could not read {description} '{}': {source}", path.display())
            }
            Self::ParseError { description, path, source } => {
                write!(f, "could not parse {description} '{}': {source}", path.display())
            }
            Self::TypeError { description, field, expected_type } => {
                write!(
                    f,
                    "{description} field '{}' has wrong type, expected {}",
                    field,
                    expected_type
                )
            }
        }
    }
}

impl JsonDoc {
    fn field<'a>(&'a self, name: &'static str) -> Result<&'a serde_json::Value, JsonError> {
        match self.value.get(name) {
            Some(value) => Ok(value),
            None => Err(JsonError::TypeError {
                description: self.description,
                field: name,
                expected_type: "any (field not found)",
            }),
        }
    }

    pub fn string<'a>(&'a self, name: &'static str) -> Result<&'a str, JsonError> {
        let value = self.field(name)?;
        match value.as_str() {
            Some(value) => Ok(value),
            None => Err(JsonError::TypeError {
                description: self.description,
                field: name,
                expected_type: "string",
            }),
        }
    }

    pub fn bool(&self, name: &'static str) -> Result<bool, JsonError> {
        let value = self.field(name)?;
        match value.as_bool() {
            Some(value) => Ok(value),
            None => Err(JsonError::TypeError {
                description: self.description,
                field: name,
                expected_type: "boolean",
            }),
        }
    }

    // unlike u64(), this does not accept stringified unsigned integers
    pub fn u64_strict(&self, name: &'static str) -> Result<u64, JsonError> {
        let value = self.field(name)?;
        match value.as_u64() {
            Some(value) => Ok(value),
            None => Err(JsonError::TypeError {
                description: self.description,
                field: name,
                expected_type: "u64",
            }),
        }
    }

    pub fn u64(&self, name: &'static str) -> Result<u64, JsonError> {
        match self.field(name)?.as_u64() {
            Some(value) => Ok(value),
            None => {
                let text = match self.field(name)?.as_str() {
                    Some(text) => text,
                    None => {
                        return Err(JsonError::TypeError {
                            description: self.description,
                            field: name,
                            expected_type: "u64",
                        });
                    }
                };

                match text.parse::<u64>() {
                    Ok(value) => Ok(value),
                    Err(_) => Err(JsonError::TypeError {
                        description: self.description,
                        field: name,
                        expected_type: "u64",
                    }),
                }
            }
        }
    }

    pub fn u8(&self, name: &'static str) -> Result<u8, JsonError> {
        let value = match self.u64(name) {
            Ok(value) => value,
            Err(err) => return Err(err),
        };

        match u8::try_from(value) {
            Ok(value) => Ok(value),
            Err(_) => Err(JsonError::TypeError {
                description: self.description,
                field: name,
                expected_type: "u8",
            }),
        }
    }
}

pub fn read(description: &'static str, path: &Path) -> Result<JsonDoc, JsonError> {
    let text = match fs::read_to_string(path) {
        Ok(the_text) => Ok(the_text),
        Err(source) => Err(JsonError::ReadError {
            description,
            path: path.to_path_buf(),
            source,
        }),
    }?;
    match serde_json::from_str(&text) {
        Ok(value) => Ok(JsonDoc { description, value }),
        Err(source) => Err(
            JsonError::ParseError { description, path: path.to_path_buf(), source }
        ),
    }
}

pub fn read_platform_config(
    description: &'static str,
    path: &Path,
) -> Result<PlatformConfig, JsonError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(source) => {
            return Err(JsonError::ReadError {
                description,
                path: path.to_path_buf(),
                source,
            });
        }
    };

    match serde_json::from_str(&text) {
        Ok(platform_config) => Ok(platform_config),
        Err(source) => Err(JsonError::ParseError {
            description,
            path: path.to_path_buf(),
            source,
        }),
    }
}
