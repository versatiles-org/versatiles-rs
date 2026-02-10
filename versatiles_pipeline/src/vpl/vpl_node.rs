//! VPL node definition.
//!
//! This module defines [`VPLNode`], the parsed building block of the VersaTiles
//! Pipeline Language (VPL). A node has a `name`, a multi-valued parameter map
//! (`properties`), and zero or more child pipelines (`sources`). Helpers convert
//! stringly-typed values to typed parameters with clear, contextual errors.

use super::VPLPipeline;
use crate::vpl::parse_vpl;
use anyhow::{Result, anyhow, ensure};
use std::{collections::BTreeMap, fmt::Debug, str::FromStr};
use versatiles_derive::context;

/// A single operation node in a VPL pipeline.
///
/// `VPLNode` holds the operation `name`, a multi-valued `properties` map (each key may
/// have one or more string values), and a list of child pipelines in `sources`.
/// Parsing/lookup helpers provide typed access (string/boolean/numeric/enum and fixed-size
/// numeric arrays) and generate consistent error messages via the `#[context]` macro.
#[derive(Clone, PartialEq)]
pub struct VPLNode {
	/// Operation/tag name, e.g., "read", "filter", or a custom transform.
	pub name: String,
	/// Multi-valued parameter map: each key maps to one or more raw string values.
	pub properties: BTreeMap<String, Vec<String>>,
	/// Zero or more child pipelines (nested VPL blocks) used as this node's inputs.
	pub sources: Vec<VPLPipeline>,
}

#[allow(dead_code)]
impl VPLNode {
	/// Parses a single-node VPL string into a `VPLNode` (asserts exactly one node).
	///
	/// Useful in tests and small utilities. Fails with rich context on invalid VPL.
	#[context("Failed to parse VPL node from string '{vpl}'")]
	pub fn try_from_str(vpl: &str) -> Result<Self> {
		let mut pipeline = parse_vpl(vpl)?;
		assert_eq!(pipeline.len(), 1);
		pipeline.pop().ok_or(anyhow!("pipeline is empty"))
	}

	/// Returns the raw value vector for `field`, if present.
	fn get_property_vec(&self, field: &str) -> Option<&Vec<String>> {
		self.properties.get(field)
	}

	/// Returns the single value for `field` or `Ok(None)` if absent; errors if multiple values exist.
	#[context("Failed to get property '{field}' from VPL node '{}'", self.name)]
	fn get_property(&self, field: &str) -> Result<Option<&String>> {
		self.properties.get(field).map_or(Ok(None), |list| {
			ensure!(
				list.len() == 1,
				"In operation '{}' the parameter '{field}' must be a single value, e.g. {field}=value",
				self.name
			);
			Ok(list.first())
		})
	}

	/// Returns all property names present on this node.
	pub fn get_property_names(&self) -> Vec<String> {
		self.properties.keys().cloned().collect()
	}

	/// Attempts to parse `field` as an enum (`T: TryFrom<&str>`), returning `Ok(None)` if absent.
	///
	/// Error messages include the node name and the invalid value.
	#[context("Failed to get optional property enum '{field}' from VPL node '{}'", self.name)]
	pub fn get_property_enum_option<'a, T>(&'a self, field: &str) -> Result<Option<T>>
	where
		T: TryFrom<&'a str>,
		<T as TryFrom<&'a str>>::Error: std::fmt::Display + Send + Sync + 'static,
	{
		self.get_property(field)?.map_or(Ok(None), move |v| {
			T::try_from(v).map(Some).map_err(|e| {
				anyhow!(
					"In operation '{}' the parameter '{field}' has an invalid value: {}",
					self.name,
					e
				)
			})
		})
	}

	/// Optional string parameter accessor; clones the stored value when present.
	#[context("Failed to get optional property string '{field}' from VPL node '{}'", self.name)]
	pub fn get_property_string_option(&self, field: &str) -> Result<Option<String>> {
		Ok(self.get_property(field)?.cloned())
	}

	/// Required string parameter accessor; errors if the field is missing.
	#[context("Failed to get required property string '{field}' from VPL node '{}'", self.name)]
	pub fn get_property_string_required(&self, field: &str) -> Result<String> {
		self.required(field, self.get_property_string_option(field))
	}

	/// Required boolean parameter accessor; accepts `1/true/yes/ok` (case-insensitive) for `true`.
	#[context("Failed to get required property bool '{field}' from VPL node '{}'", self.name)]
	pub fn get_property_bool_required(&self, field: &str) -> Result<bool> {
		self.required(field, self.get_property_bool_option(field))
	}

	/// Optional boolean parameter accessor; accepts `1/true/yes/ok` (case-insensitive).
	#[context("Failed to get optional property bool '{field}' from VPL node '{}'", self.name)]
	pub fn get_property_bool_option(&self, field: &str) -> Result<Option<bool>> {
		Ok(self
			.get_property(field)?
			.map(|v| matches!(v.trim().to_lowercase().as_str(), "1" | "true" | "yes" | "ok")))
	}

	/// Optional numeric parameter accessor using `FromStr` for the target type.
	#[context("Failed to get optional property number '{field}' from VPL node '{}'", self.name)]
	pub fn get_property_number_option<T>(&self, field: &str) -> Result<Option<T>>
	where
		T: FromStr,
		<T as FromStr>::Err: std::error::Error + Send + Sync + 'static,
	{
		self.get_property(field)?.map_or(Ok(None), |v| {
			v.parse::<T>().map(Some).map_err(|_| {
				anyhow!(
					"In operation '{}' the parameter '{field}' must be a number, e.g. {field}=42 — got '{v}'",
					self.name
				)
			})
		})
	}

	/// Required numeric parameter accessor; errors when missing or when parsing fails.
	#[context("Failed to get required property number '{field}' from VPL node '{}'", self.name)]
	pub fn get_property_number_required<T>(&self, field: &str) -> Result<T>
	where
		T: FromStr,
		<T as FromStr>::Err: std::error::Error + Send + Sync + 'static,
	{
		self.required(field, self.get_property_number_option::<T>(field))
	}

	/// Optional fixed-size numeric array accessor; enforces exactly `N` elements.
	#[context("Failed to get optional property number array '{field}' from VPL node '{}'", self.name)]
	pub fn get_property_number_array_option<T, const N: usize>(&self, field: &str) -> Result<Option<[T; N]>>
	where
		T: FromStr + Debug,
		<T as FromStr>::Err: std::error::Error + Send + Sync + 'static,
	{
		Ok(if let Some(vec) = self.get_property_vec(field) {
			ensure!(
				vec.len() == N,
				"In operation '{}' the parameter '{field}' must be an array of {N} numbers, e.g. {field}=[1,2,...,{N}]",
				self.name
			);
			Some(
				vec.iter()
					.map(|s| {
						s.parse::<T>().map_err(|_| {
							anyhow!(
								"In operation '{}' the parameter '{field}': failed to parse element '{s}' as a number",
								self.name
							)
						})
					})
					.collect::<Result<Vec<_>>>()?
					.try_into()
					.unwrap(),
			)
		} else {
			None
		})
	}

	/// Required fixed-size numeric array accessor; enforces presence and exactly `N` elements.
	#[context("Failed to get required property number array '{field}' from VPL node '{}'", self.name)]
	pub fn get_property_number_array_required<T, const N: usize>(&self, field: &str) -> Result<[T; N]>
	where
		T: FromStr + Debug,
		<T as FromStr>::Err: std::error::Error + Send + Sync + 'static,
	{
		self.required(field, self.get_property_number_array_option::<T, N>(field))
	}

	/// Internal helper: converts `Ok(Some(_))` to the value or produces a standard "required" error.
	fn required<T>(&self, field: &str, result: Result<Option<T>>) -> Result<T> {
		result?.ok_or_else(|| {
			anyhow!(
				"In operation '{}' the parameter '{}' is required but missing.",
				self.name,
				field
			)
		})
	}
}

/// Creates a node with the given name and no properties/sources.
impl From<&str> for VPLNode {
	fn from(name: &str) -> Self {
		VPLNode {
			name: name.to_string(),
			properties: BTreeMap::new(),
			sources: vec![],
		}
	}
}

/// Creates a node with one `(key, value)` property.
impl From<(&str, (&str, &str))> for VPLNode {
	fn from(input: (&str, (&str, &str))) -> Self {
		VPLNode {
			name: input.0.to_string(),
			properties: make_property(vec![input.1]),
			sources: vec![],
		}
	}
}

/// Creates a node with multiple `(key, value)` properties.
impl From<(&str, Vec<(&str, &str)>)> for VPLNode {
	fn from(input: (&str, Vec<(&str, &str)>)) -> Self {
		VPLNode {
			name: input.0.to_string(),
			properties: make_property(input.1),
			sources: vec![],
		}
	}
}

/// Creates a node with properties and a single child pipeline.
impl From<(&str, Vec<(&str, &str)>, VPLPipeline)> for VPLNode {
	fn from(input: (&str, Vec<(&str, &str)>, VPLPipeline)) -> Self {
		VPLNode {
			name: input.0.to_string(),
			properties: make_property(input.1),
			sources: vec![input.2],
		}
	}
}

/// Creates a node with properties and multiple child pipelines.
impl From<(&str, Vec<(&str, &str)>, Vec<VPLPipeline>)> for VPLNode {
	fn from(input: (&str, Vec<(&str, &str)>, Vec<VPLPipeline>)) -> Self {
		VPLNode {
			name: input.0.to_string(),
			properties: make_property(input.1),
			sources: input.2,
		}
	}
}

impl Debug for VPLNode {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let mut s = f.debug_struct("VPLNode");
		s.field("name", &self.name);
		if !self.properties.is_empty() {
			s.field("properties", &self.properties);
		}
		if !self.sources.is_empty() {
			s.field("sources", &self.sources);
		}
		s.finish()
	}
}

#[cfg(test)]
fn make_properties(input: Vec<(&str, Vec<&str>)>) -> BTreeMap<String, Vec<String>> {
	input
		.into_iter()
		.map(|(k, v)| {
			(
				k.to_string(),
				v.into_iter().map(std::string::ToString::to_string).collect(),
			)
		})
		.collect()
}

fn make_property(input: Vec<(&str, &str)>) -> BTreeMap<String, Vec<String>> {
	input
		.into_iter()
		.map(|(k, v)| (k.to_string(), vec![v.to_string()]))
		.collect()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_vplnode_get_property() {
		let node = VPLNode {
			name: "node".to_string(),
			properties: make_property(vec![("key1", "value1"), ("key2", "value2")]),
			sources: vec![],
		};
		assert_eq!(node.get_property_vec("key1").unwrap(), &vec!["value1".to_string()]);
		assert_eq!(node.get_property_vec("key2").unwrap(), &vec!["value2".to_string()]);
		assert!(node.get_property_vec("key3").is_none());
	}

	#[test]
	fn test_vplnode_get_property_string() {
		let node = VPLNode {
			name: "node".to_string(),
			properties: make_property(vec![("key1", "value1")]),
			sources: vec![],
		};
		assert_eq!(
			node.get_property_string_option("key1").unwrap().unwrap(),
			"value1".to_string()
		);
		assert!(node.get_property_string_option("key2").unwrap().is_none());
	}

	#[test]
	fn test_vplnode_get_property_string_req() {
		let node = VPLNode {
			name: "node".to_string(),
			properties: make_property(vec![("key1", "value1")]),
			sources: vec![],
		};
		assert_eq!(node.get_property_string_required("key1").unwrap(), "value1".to_string());
		assert!(node.get_property_string_required("key2").is_err());
	}

	#[test]
	fn test_vplnode_get_property_bool_req() {
		let node = VPLNode {
			name: "node".to_string(),
			properties: make_property(vec![("key1", "true"), ("key2", "0")]),
			sources: vec![],
		};
		assert!(node.get_property_bool_required("key1").unwrap());
		assert!(!node.get_property_bool_required("key2").unwrap());
	}

	#[test]
	fn test_vplnode_get_property_number() {
		let node = VPLNode {
			name: "node".to_string(),
			properties: make_property(vec![("key1", "42"), ("key2", "invalid")]),
			sources: vec![],
		};
		assert_eq!(node.get_property_number_option::<i32>("key1").unwrap().unwrap(), 42);
		assert!(node.get_property_number_option::<i32>("key2").is_err());
		assert!(node.get_property_number_option::<i32>("key3").unwrap().is_none());
	}

	#[test]
	fn test_vplnode_get_property_number_req() {
		let node = VPLNode {
			name: "node".to_string(),
			properties: make_property(vec![("key1", "42")]),
			sources: vec![],
		};
		assert_eq!(node.get_property_number_required::<i32>("key1").unwrap(), 42);
		assert!(node.get_property_number_required::<i32>("key2").is_err());
	}

	#[test]
	fn test_vplnode_get_property_number_array4() {
		let node = VPLNode {
			name: "node".to_string(),
			properties: make_properties(vec![("key1", vec!["1", "2", "3", "4"])]),
			sources: vec![],
		};
		assert_eq!(
			node
				.get_property_number_array_option::<i32, 4>("key1")
				.unwrap()
				.unwrap(),
			[1, 2, 3, 4]
		);
		assert!(
			node
				.get_property_number_array_option::<i32, 4>("key2")
				.unwrap()
				.is_none()
		);
	}

	#[test]
	fn test_vplnode_get_property_number_array4_req() {
		let node = VPLNode {
			name: "node".to_string(),
			properties: make_properties(vec![("key1", vec!["1", "2", "3", "4"])]),
			sources: vec![],
		};
		assert_eq!(
			node.get_property_number_array_required::<i32, 4>("key1").unwrap(),
			[1, 2, 3, 4]
		);
		assert!(node.get_property_number_array_required::<i32, 4>("key2").is_err());
	}

	#[test]
	fn test_vplnode_required() -> Result<()> {
		let node = VPLNode {
			name: "node".to_string(),
			properties: make_property(vec![("key1", "value1")]),
			sources: vec![],
		};
		assert_eq!(
			node.required("key1", Ok(Some("value1".to_string())))?,
			"value1".to_string()
		);
		assert!(node.required::<String>("key2", Ok(None)).is_err());
		Ok(())
	}

	#[test]
	fn test_vplnode_from_str() {
		fn run(vpl: &str) {
			let node = VPLNode::try_from_str(vpl).unwrap();

			assert_eq!(node.name, "node");
			assert_eq!(node.properties.get("key1").unwrap(), &vec!["value1".to_string()]);
			assert_eq!(
				node.properties.get("key2").unwrap(),
				&vec![String::from("1"), String::from("2"), String::from("3")]
			);
			assert_eq!(node.sources[0].pipeline[0].name, "child");
		}

		run(
			r#" node key1 = "value1" key2 = [1,"2",3] [
   child
 ]
		"#,
		);

		run(r#" node key1 = "value1" key2 = [ 1 , "2" , 3 ] [ child ] "#);
		run(r#"node key1="value1" key2=[1,"2",3][child]"#);
	}

	#[test]
	fn test_debug_impl() {
		let node = VPLNode {
			name: "test_node".to_string(),
			properties: make_properties(vec![("key1", vec!["value1", "value2"]), ("key2", vec!["value3"])]),
			sources: vec![VPLPipeline::default()],
		};
		let debug_str = format!("{node:?}");
		assert!(debug_str.contains("VPLNode"));
		assert!(debug_str.contains("test_node"));
		assert!(debug_str.contains("properties"));
		assert!(debug_str.contains("sources"));
	}

	#[test]
	fn test_from_str() {
		let node: VPLNode = "test_node".into();
		assert_eq!(node.name, "test_node");
		assert!(node.properties.is_empty());
		assert!(node.sources.is_empty());
	}

	#[test]
	fn test_from_tuple_str_str() {
		let node: VPLNode = ("test_node", ("key", "value")).into();
		assert_eq!(node.name, "test_node");
		assert_eq!(node.properties.get("key").unwrap(), &vec!["value".to_string()]);
		assert!(node.sources.is_empty());
	}

	#[test]
	fn test_from_tuple_str_vec_tuple_str_str() {
		let node: VPLNode = ("test_node", vec![("key1", "value1"), ("key2", "value2")]).into();
		assert_eq!(node.name, "test_node");
		assert_eq!(node.properties.get("key1").unwrap(), &vec!["value1".to_string()]);
		assert_eq!(node.properties.get("key2").unwrap(), &vec!["value2".to_string()]);
		assert!(node.sources.is_empty());
	}

	#[test]
	fn test_from_tuple_str_vec_tuple_str_str_pipeline() {
		let pipeline = VPLPipeline::default();
		let node: VPLNode = (
			"test_node",
			vec![("key1", "value1"), ("key2", "value2")],
			pipeline.clone(),
		)
			.into();
		assert_eq!(node.name, "test_node");
		assert_eq!(node.properties.get("key1").unwrap(), &vec!["value1".to_string()]);
		assert_eq!(node.properties.get("key2").unwrap(), &vec!["value2".to_string()]);
		assert_eq!(node.sources, vec![pipeline]);
	}

	#[test]
	fn test_from_tuple_str_vec_tuple_str_str_vec_pipeline() {
		let pipeline1 = VPLPipeline::default();
		let pipeline2 = VPLPipeline::default();
		let node: VPLNode = (
			"test_node",
			vec![("key1", "value1"), ("key2", "value2")],
			vec![pipeline1.clone(), pipeline2.clone()],
		)
			.into();
		assert_eq!(node.name, "test_node");
		assert_eq!(node.properties.get("key1").unwrap(), &vec!["value1".to_string()]);
		assert_eq!(node.properties.get("key2").unwrap(), &vec!["value2".to_string()]);
		assert_eq!(node.sources, vec![pipeline1, pipeline2]);
	}
}
