use crate::container::composer::operations::{READERS, TRANSFORMERS};
use itertools::Itertools;

pub fn get_composer_operation_docs() -> String {
	let readers = READERS
		.iter()
		.map(|b| format!("## Reader **{}**:\n\n{}", b.get_id(), b.get_docs()))
		.join("\n\n")
		.to_string();

	let transformers = TRANSFORMERS
		.iter()
		.map(|b| format!("## Transform **{}**:\n\n{}", b.get_id(), b.get_docs()))
		.join("\n\n")
		.to_string();

	return format!("# Readers\n\n{readers}\n# Transformers\n\n{transformers}");
}
