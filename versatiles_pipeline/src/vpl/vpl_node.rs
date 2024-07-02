use super::VPLPipeline;
use crate::vpl::parse_vpl;
use anyhow::{anyhow, ensure, Result};
use std::{collections::HashMap, fmt::Debug, str::FromStr};

#[derive(Clone, PartialEq)]
pub struct VPLNode {
	pub name: String,
	pub properties: HashMap<String, Vec<String>>,
	pub sources: Vec<VPLPipeline>,
}

#[allow(dead_code)]
impl VPLNode {
	pub fn from_str(vpl: &str) -> Result<Self> {
		let mut pipeline = parse_vpl(vpl)?;
		assert_eq!(pipeline.len(), 1);
		pipeline.pop().ok_or(anyhow!("pipeline is empty"))
	}

	fn get_property_vec(&self, field: &str) -> Option<&Vec<String>> {
		self.properties.get(field)
	}

	fn get_property(&self, field: &str) -> Result<Option<&String>> {
		self.properties.get(field).map_or(Ok(None), |list| {
			ensure!(
				list.len() == 1,
				"In operation '{}' the parameter '{field}' must have exactly one entry.",
				self.name
			);
			Ok(list.first())
		})
	}

	pub fn get_property_string(&self, field: &str) -> Result<Option<String>> {
		Ok(self.get_property(field)?.map(|v| v.to_string()))
	}

	pub fn get_property_string_req(&self, field: &str) -> Result<String> {
		self.required(field, self.get_property_string(field))
	}

	pub fn get_property_bool_req(&self, field: &str) -> Result<bool> {
		Ok(self.get_property(field)?.map_or(false, |v| {
			matches!(
				v.trim().to_lowercase().as_str(),
				"1" | "true" | "yes" | "ok"
			)
		}))
	}

	pub fn get_property_number<T>(&self, field: &str) -> Result<Option<T>>
	where
		T: FromStr,
		<T as FromStr>::Err: std::error::Error + Send + Sync + 'static,
	{
		self
			.get_property(field)?
			.map_or(Ok(None), |v| v.parse::<T>().map(Some).map_err(Into::into))
	}

	pub fn get_property_number_req(&self, field: &str) -> Result<String> {
		self.required(field, self.get_property_number(field))
	}

	pub fn get_property_number_array4<T>(&self, field: &str) -> Result<Option<[T; 4]>>
	where
		T: FromStr,
		<T as FromStr>::Err: std::error::Error + Send + Sync + 'static,
	{
		Ok(if let Some(vec) = self.get_property_vec(field) {
			ensure!(
				vec.len() == 4,
				"In operation '{}' the parameter '{field}' must be an array of 4 numbers.",
				self.name
			);
			Some([
				vec[0].parse::<T>()?,
				vec[1].parse::<T>()?,
				vec[2].parse::<T>()?,
				vec[3].parse::<T>()?,
			])
		} else {
			None
		})
	}

	pub fn get_property_number_array4_req<T>(&self, field: &str) -> Result<[T; 4]>
	where
		T: FromStr,
		<T as FromStr>::Err: std::error::Error + Send + Sync + 'static,
	{
		self.required(field, self.get_property_number_array4(field))
	}

	fn required<T>(&self, field: &str, result: Result<Option<T>>) -> Result<T> {
		result?.ok_or_else(|| {
			anyhow!(
				"In operation '{}' the parameter '{}' is required.",
				self.name,
				field
			)
		})
	}
}

impl From<&str> for VPLNode {
	fn from(name: &str) -> Self {
		VPLNode {
			name: name.to_string(),
			properties: HashMap::new(),
			sources: vec![],
		}
	}
}

fn make_properties(input: Vec<(&str, &str)>) -> HashMap<String, Vec<String>> {
	input
		.iter()
		.map(|(k, v)| (k.to_string(), vec![v.to_string()]))
		.collect()
}

impl From<(&str, (&str, &str))> for VPLNode {
	fn from(input: (&str, (&str, &str))) -> Self {
		VPLNode {
			name: input.0.to_string(),
			properties: make_properties(vec![input.1]),
			sources: vec![],
		}
	}
}

impl From<(&str, Vec<(&str, &str)>)> for VPLNode {
	fn from(input: (&str, Vec<(&str, &str)>)) -> Self {
		VPLNode {
			name: input.0.to_string(),
			properties: make_properties(input.1),
			sources: vec![],
		}
	}
}

impl From<(&str, Vec<(&str, &str)>, VPLPipeline)> for VPLNode {
	fn from(input: (&str, Vec<(&str, &str)>, VPLPipeline)) -> Self {
		VPLNode {
			name: input.0.to_string(),
			properties: make_properties(input.1),
			sources: vec![input.2],
		}
	}
}

impl From<(&str, Vec<(&str, &str)>, Vec<VPLPipeline>)> for VPLNode {
	fn from(input: (&str, Vec<(&str, &str)>, Vec<VPLPipeline>)) -> Self {
		VPLNode {
			name: input.0.to_string(),
			properties: make_properties(input.1),
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
mod tests {
	use super::*;

	fn make_properties(input: &[(&str, &str)]) -> HashMap<String, Vec<String>> {
		input
			.iter()
			.map(|(k, v)| (k.to_string(), vec![v.to_string()]))
			.collect()
	}

	#[test]
	fn test_vplnode_get_property() -> Result<()> {
		let node = VPLNode {
			name: "node".to_string(),
			properties: make_properties(&[("key1", "value1"), ("key2", "value2")]),
			sources: vec![],
		};
		assert_eq!(
			node.get_property_vec("key1").unwrap(),
			&vec!["value1".to_string()]
		);
		assert_eq!(
			node.get_property_vec("key2").unwrap(),
			&vec!["value2".to_string()]
		);
		assert!(node.get_property_vec("key3").is_none());
		Ok(())
	}
}
