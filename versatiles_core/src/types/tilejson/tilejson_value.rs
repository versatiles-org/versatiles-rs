use crate::{json::JsonValue, utils::float_to_int};
use anyhow::{Result, bail};

/// An enumeration representing allowed JSON value types in this module.
#[derive(Clone, Debug, PartialEq)]
pub enum TileJsonValue {
	/// A list of strings (originally from a JSON array).
	List(Vec<String>),
	/// A single string (originally from a JSON string).
	String(String),
	/// A single integer (stored as `i64`).
	Integer(i64),
}

impl TileJsonValue {
	/// Returns `Some(&str)` if the value is a string, or `None` otherwise.
	pub fn as_str(&self) -> Option<&str> {
		match self {
			TileJsonValue::String(s) => Some(s),
			_ => None,
		}
	}

	/// Returns `Some(i64)` if the value is a byte, or `None` otherwise.
	pub fn as_integer(&self) -> Option<i64> {
		match self {
			TileJsonValue::Integer(b) => Some(*b),
			_ => None,
		}
	}

	/// Converts this `TileJsonValue` into a generic [`JsonValue`].
	pub fn as_json_value(&self) -> JsonValue {
		match self {
			TileJsonValue::Integer(b) => JsonValue::from(*b),
			TileJsonValue::List(l) => JsonValue::from(l),
			TileJsonValue::String(s) => JsonValue::from(s),
		}
	}

	/// Returns a string describing which variant this `TileJsonValue` is (`"List"`, `"String"`, or `"Integer"`).
	pub fn type_name(&self) -> &str {
		match self {
			TileJsonValue::Integer(_) => "Integer",
			TileJsonValue::List(_) => "List",
			TileJsonValue::String(_) => "String",
		}
	}

	/// Returns `true` if this value is a `TileJsonValue::List`.
	pub fn is_list(&self) -> bool {
		matches!(self, TileJsonValue::List(_))
	}

	/// Returns `true` if this value is a `TileJsonValue::String`.
	pub fn is_string(&self) -> bool {
		matches!(self, TileJsonValue::String(_))
	}

	/// Returns `true` if this value is a `TileJsonValue::Integer`.
	pub fn is_integer(&self) -> bool {
		matches!(self, TileJsonValue::Integer(_))
	}
}

impl From<u8> for TileJsonValue {
	fn from(value: u8) -> Self {
		TileJsonValue::Integer(i64::from(value))
	}
}

impl TryFrom<&JsonValue> for TileJsonValue {
	type Error = anyhow::Error;

	/// Attempts to convert a reference to a [`JsonValue`] into a [`TileJsonValue`].
	///
	/// # Errors
	///
	/// Returns an error if:
	/// - The `JsonValue` is out of range for a byte (`u8`).
	/// - The `JsonValue` is some other type not supported by [`TileJsonValue`].
	fn try_from(value: &JsonValue) -> Result<Self> {
		match value {
			JsonValue::String(s) => Ok(TileJsonValue::String(s.to_owned())),
			JsonValue::Array(a) => Ok(TileJsonValue::List(a.to_string_vec()?)),
			JsonValue::Number(n) => Ok(TileJsonValue::Integer(float_to_int(*n)?)),
			_ => bail!("Invalid value type: only string, array, or integer allowed"),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn as_str_only_matches_string_variant() {
		assert_eq!(TileJsonValue::String("hi".into()).as_str(), Some("hi"));
		assert_eq!(TileJsonValue::Integer(7).as_str(), None);
		assert_eq!(TileJsonValue::List(vec!["a".into()]).as_str(), None);
	}

	#[test]
	fn as_integer_only_matches_integer_variant() {
		assert_eq!(TileJsonValue::Integer(7).as_integer(), Some(7));
		assert_eq!(TileJsonValue::String("x".into()).as_integer(), None);
	}

	#[test]
	fn type_name_reports_variant() {
		assert_eq!(TileJsonValue::Integer(1).type_name(), "Integer");
		assert_eq!(TileJsonValue::String("x".into()).type_name(), "String");
		assert_eq!(TileJsonValue::List(vec![]).type_name(), "List");
	}

	#[test]
	fn is_flags() {
		let i = TileJsonValue::Integer(1);
		let s = TileJsonValue::String("x".into());
		let l = TileJsonValue::List(vec![]);
		assert!(i.is_integer() && !i.is_string() && !i.is_list());
		assert!(s.is_string() && !s.is_integer() && !s.is_list());
		assert!(l.is_list() && !l.is_string() && !l.is_integer());
	}

	#[test]
	fn as_json_value_and_from_u8() {
		let v = TileJsonValue::from(5_u8);
		assert_eq!(v.as_json_value(), JsonValue::from(5_i64));
		let s: JsonValue = TileJsonValue::String("x".into()).as_json_value();
		assert_eq!(s, JsonValue::from("x"));
		let l: JsonValue = TileJsonValue::List(vec!["a".into(), "b".into()]).as_json_value();
		assert_eq!(l, JsonValue::from(vec!["a", "b"]));
	}

	#[test]
	fn try_from_json_value_accepts_supported_and_rejects_others() {
		assert!(TileJsonValue::try_from(&JsonValue::from("x")).is_ok());
		assert!(TileJsonValue::try_from(&JsonValue::from(42_i64)).is_ok());
		assert!(TileJsonValue::try_from(&JsonValue::from(vec!["a", "b"])).is_ok());
		assert!(TileJsonValue::try_from(&JsonValue::Null).is_err());
		assert!(TileJsonValue::try_from(&JsonValue::Boolean(true)).is_err());
	}
}
