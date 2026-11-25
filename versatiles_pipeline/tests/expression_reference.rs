//! Keeps the function list in `vector_filter_features`' docs in step with `expression_operators()`.
//!
//! The docs become the operation reference in `versatiles_pipeline/README.md` and
//! `versatiles help pipeline`. They used to send readers to the CEL spec for the functions, which
//! lists some this build doesn't have. Now they name the ones it has, and this test fails when the
//! paragraph and the table disagree in either direction.

#![cfg(feature = "codegen")]

use std::collections::BTreeSet;

use versatiles_pipeline::{ExpressionOperatorKind, all_operation_metadata, expression_operators};

/// The names in backticks in the paragraph that lists the functions.
fn names_in_the_reference() -> BTreeSet<String> {
	let op = all_operation_metadata()
		.into_iter()
		.find(|op| op.tag_name == "vector_filter_features")
		.expect("vector_filter_features is registered");
	let paragraph = op
		.details
		.split("\n\n")
		.find(|paragraph| paragraph.contains("can call the functions"))
		.expect("the details list the functions");
	let listed = &paragraph[paragraph.find("can call").expect("the paragraph says what it lists")..];
	listed.split('`').skip(1).step_by(2).map(str::to_string).collect()
}

#[test]
fn the_reference_names_every_function_method_and_macro() {
	let in_table: BTreeSet<String> = expression_operators()
		.iter()
		.filter(|op| {
			matches!(
				op.kind,
				ExpressionOperatorKind::Function | ExpressionOperatorKind::Method | ExpressionOperatorKind::Macro
			)
		})
		.map(|op| op.written.to_string())
		.collect();
	assert_eq!(names_in_the_reference(), in_table);
}
