use crate::container::composer::operations::{READABLES, TRANSFORMS};
use itertools::Itertools;

pub fn get_composer_operation_docs() -> String {
	let readables = READABLES
		.iter()
		.map(|b| format!("Reader \"{}\":\n\n{}", b.get_id(), b.get_docs()));

	let transforms = TRANSFORMS
		.iter()
		.map(|b| format!("Transform \"{}\":\n\n{}", b.get_id(), b.get_docs()));

	return readables.chain(transforms).join("\n\n").to_string();
}
