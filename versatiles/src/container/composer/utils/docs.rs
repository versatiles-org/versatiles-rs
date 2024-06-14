use crate::container::composer::{
	operations::{READERS, TRANSFORMERS},
	BuilderTrait,
};
use itertools::Itertools;

pub fn get_composer_operation_docs() -> String {
	return vec![
		vec2string("Readers", &READERS),
		vec2string("Transformers", &TRANSFORMERS),
	]
	.join("\n\n");

	fn vec2string<T>(title: &str, list: &Vec<Box<T>>) -> String
	where
		T: BuilderTrait + ?Sized,
	{
		return format!(
			"# {title}\n\n{}",
			list
				.iter()
				.map(|b| format!("## {}:\n\n{}", b.get_id(), b.get_docs()))
				.sorted()
				.join("\n\n")
				.to_string()
		);
	}
}
