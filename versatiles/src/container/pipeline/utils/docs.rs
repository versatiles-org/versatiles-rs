use crate::container::pipeline::{
	operations::{READERS, TRANSFORMERS},
	BuilderTrait,
};
use itertools::Itertools;

pub fn get_pipeline_operation_docs() -> String {
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
				.sorted_by_key(|b| b.get_id())
				.map(|b| format!("## {}:\n{}", b.get_id(), b.get_docs()))
				.join("\n\n")
				.to_string()
		);
	}
}
