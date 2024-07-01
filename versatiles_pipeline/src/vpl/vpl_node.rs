use super::VPLPipeline;
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
	fn get_property_vec(&self, field: &str) -> Option<&Vec<String>> {
		self.properties.get(field)
	}

	fn get_property0(&self, field: &str) -> Result<Option<&String>> {
		self.properties.get(field).map_or(Ok(None), |list| {
			ensure!(
				list.len() == 1,
				"In operation '{}' the parameter '{field}' must have exactly one entry.",
				self.name
			);
			Ok(list.first())
		})
	}

	fn get_property1(&self, field: &str) -> Result<&String> {
		self.get_property0(field)?.ok_or_else(|| {
			anyhow!(
				"In operation '{}' the parameter '{field}' is required.",
				self.name
			)
		})
	}

	pub fn get_property_string0(&self, field: &str) -> Result<Option<String>> {
		Ok(self.get_property0(field)?.map(|v| v.to_string()))
	}

	pub fn get_property_string1(&self, field: &str) -> Result<String> {
		self.get_property1(field).map(|v| v.to_string())
	}

	pub fn get_property_bool(&self, field: &str) -> Result<bool> {
		Ok(self.get_property0(field)?.map_or(false, |v| {
			matches!(
				v.trim().to_lowercase().as_str(),
				"1" | "true" | "yes" | "ok"
			)
		}))
	}

	pub fn get_property_number0<T>(&self, field: &str) -> Result<Option<T>>
	where
		T: FromStr,
		<T as FromStr>::Err: std::error::Error + Send + Sync + 'static,
	{
		self
			.get_property0(field)?
			.map_or(Ok(None), |v| v.parse::<T>().map(Some).map_err(Into::into))
	}

	pub fn get_property_number_array4<T>(&self, field: &str) -> Result<Option<[T; 4]>>
	where
		T: FromStr,
		<T as FromStr>::Err: std::error::Error + Send + Sync + 'static,
	{
		Ok(if let Some(vec) = self.get_property_vec(field) {
			ensure!(
				vec.len() == 4,
				"In operation '{}' the parameter '{field}' must have 4 values.",
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
