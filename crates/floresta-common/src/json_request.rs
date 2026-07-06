// SPDX-License-Identifier: MIT OR Apache-2.0

use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use core::error;
use core::fmt;
use core::prelude::v1::Some;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

#[derive(Debug)]
pub enum RequestError {
    MissingParameter(String),
    InvalidParameterType(String),
    InvalidParameterStructure,
}

impl fmt::Display for RequestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingParameter(e) => write!(f, "Missing parameter: {e}"),
            Self::InvalidParameterType(e) => write!(f, "Invalid parameter type: {e}"),
            Self::InvalidParameterStructure => {
                write!(f, "Invalid parameter structure")
            }
        }
    }
}

impl error::Error for RequestError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::MissingParameter(_) => None,
            Self::InvalidParameterType(_) => None,
            Self::InvalidParameterStructure => None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
/// Represents a JSON-RPC request (versions 1.0 and 2.0).
pub struct Request {
    /// The JSON-RPC version, typically "2.0".
    ///
    /// For JSON-RPC 2.0, this field is required. For earlier versions, it may be omitted.
    ///
    /// Source: <`https://json-rpc.dev/docs/reference/version-diff`>
    pub jsonrpc: Option<String>,

    /// The method to be invoked, e.g., "getblock", "sendtransaction".
    pub method: String,

    /// The parameters for the method, json value that must be an array or an object.
    pub params: Option<Value>,

    /// An optional identifier for the request, which can be used to match responses.
    pub id: Value,
}

impl Request {
    /// Extracts an optional parameter from the request by position (array params) or name (object params).
    ///
    /// Returns `Ok(None)` if the field is absent or `null`.
    /// Returns an error if `params` itself is `null` or has an unexpected structure.
    pub fn get_optional<'de, T: Deserialize<'de>>(
        &self,
        index: usize,
        field_name: &str,
    ) -> Result<Option<T>, RequestError> {
        let value = match &self.params {
            Some(Value::Null) => {
                return Err(RequestError::MissingParameter(field_name.to_string()));
            }
            Some(Value::Array(values)) => values.get(index),
            Some(Value::Object(map)) => map.get(field_name),
            _ => {
                return Err(RequestError::InvalidParameterStructure);
            }
        }
        .filter(|v| !v.is_null());

        value
            .map(|value| {
                T::deserialize(value.clone())
                    .map_err(|e| RequestError::InvalidParameterType(format!("{field_name}: {e}")))
            })
            .transpose()
    }

    /// Extracts a required parameter, returning [`Error::MissingParameter`] if absent.
    pub fn get_at<'de, T: Deserialize<'de>>(
        &self,
        index: usize,
        field_name: &str,
    ) -> Result<T, RequestError> {
        self.get_optional(index, field_name)?
            .ok_or_else(|| RequestError::MissingParameter(field_name.to_string()))
    }

    /// Like [`get_optional`], but substitutes `default` instead of returning `None`.
    pub fn get_with_default<'de, T: Deserialize<'de>>(
        &self,
        index: usize,
        field_name: &str,
        default: T,
    ) -> Result<T, RequestError> {
        Ok(self.get_optional(index, field_name)?.unwrap_or(default))
    }
}
