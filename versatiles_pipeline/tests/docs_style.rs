//! Enforces the mechanical half of `DOCSTYLE.md` across every VPL operation.
//!
//! The doc comments on an operation's `Args` struct are the operation reference,
//! an editor's hover text, and the TypeScript bindings' documentation. They
//! drifted into four different ways of writing a default and two moods for a
//! summary before anyone noticed, because nothing checked them. This does.
//!
//! Only the rules a machine can judge live here. Whether a description is
//! *useful* is still a review question — see `DOCSTYLE.md` for the rest.

#![cfg(feature = "codegen")]

use versatiles_pipeline::{all_operation_metadata, vpl::VPLFieldMeta};

/// How long a parameter description may be.
///
/// One line in most terminals, and enough for what the value is plus what it
/// defaults to. See "Length" in `DOCSTYLE.md` for why the cap is the point
/// rather than a nuisance.
const MAX_DESCRIPTION_CHARS: usize = 100;

/// The prose of a description, with backticked spans removed.
///
/// Spans wrapped in backticks are VPL syntax being quoted — `filename="a.tar"`,
/// `props["key"]`, `2^z - 1 - x`, a URL used as an example value. None of the
/// prose rules apply inside them. Whitespace is collapsed afterwards, since
/// removing a span leaves the spaces that surrounded it adjacent.
fn prose_of(text: &str) -> String {
	let mut out = String::with_capacity(text.len());
	let mut inside = false;
	for ch in text.chars() {
		if ch == '`' {
			inside = !inside;
		} else if !inside {
			out.push(ch);
		}
	}
	out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether the prose carries a list marker that the renderer will flatten.
///
/// A field's doc lines are joined with a single space, so a `-` bullet survives
/// as literal text in the middle of a sentence — `…depends on point_reduction: -
/// min_distance (default): …`. A hyphen used as a hyphen never follows sentence
/// punctuation, which is what separates the two.
fn has_collapsed_list(prose: &str) -> bool {
	prose.starts_with("- ")
		|| prose
			.match_indices(" - ")
			.any(|(i, _)| i > 0 && matches!(prose.as_bytes()[i - 1], b':' | b'.' | b';'))
}

/// Collects one violation per rule broken, so a run reports everything at once
/// rather than one failure per `cargo test`.
#[derive(Default)]
struct Violations(Vec<String>);

impl Violations {
	fn check(&mut self, ok: bool, what: impl std::fmt::Display) {
		if !ok {
			self.0.push(what.to_string());
		}
	}
}

fn check_summary(op: &str, summary: &str, v: &mut Violations) {
	v.check(!summary.is_empty(), format!("{op}: has no summary"));
	if summary.is_empty() {
		return;
	}
	v.check(
		summary.ends_with('.'),
		format!("{op}: summary does not end with a period: {summary:?}"),
	);
	v.check(
		!summary.contains('\n'),
		format!("{op}: summary spans several lines; it is collapsed to one when rendered"),
	);
	v.check(
		!summary.to_lowercase().starts_with("arguments for"),
		format!("{op}: summary describes the Rust struct rather than the operation"),
	);
	// Third person, not the imperative: "Reads a container", not "Read a
	// container". Checked by the verb's `s`, which is what separates the two.
	let first = summary.split_whitespace().next().unwrap_or_default();
	v.check(
		first.ends_with('s') && first.chars().all(|c| c.is_ascii_alphabetic() || c == '-'),
		format!("{op}: summary should open with a third-person verb, found {first:?}"),
	);
	// "raster_overscale" does not need to open "Raster overscale operation — ";
	// the heading already says which operation this is. Opening with the verb the
	// operation is named after — "Filters …" for `filter` — is fine.
	let restated = format!("{} operation", op.replace('_', " "));
	v.check(
		!summary.to_lowercase().starts_with(&restated),
		format!("{op}: summary restates the operation's name"),
	);
}

fn check_details(op: &str, details: &str, v: &mut Violations) {
	v.check(
		!details.contains("### Arguments") && !details.contains("### Parameters"),
		format!("{op}: details hand-write a parameter list, which the reference already generates"),
	);
}

fn check_field(op: &str, field: &VPLFieldMeta, v: &mut Violations) {
	// The `sources` pseudo-field is rendered as its own section, not as a
	// parameter bullet, so the bullet rules do not apply to it.
	if field.is_sources {
		return;
	}
	let name = &field.name;
	let doc = field.doc.trim();
	let at = format!("{op}.{name}");

	v.check(!doc.is_empty(), format!("{at}: has no description"));
	if doc.is_empty() {
		return;
	}

	let prose = prose_of(doc);

	v.check(
		doc.chars().next().is_some_and(char::is_uppercase),
		format!("{at}: description does not start with a capital: {doc:?}"),
	);
	v.check(
		doc.ends_with('.'),
		format!("{at}: description does not end with a period: ...{:?}", tail(doc)),
	);
	v.check(
		!has_collapsed_list(&prose),
		format!("{at}: description contains list markup that collapses when rendered: {doc:?}"),
	);
	v.check(
		!prose.contains("http://") && !prose.contains("https://"),
		format!("{at}: description carries a link; links belong in the operation's details"),
	);
	// The generated bullet already prints the type and (required)/(optional).
	v.check(
		!prose.contains("(required)") && !prose.contains("(optional)") && !prose.contains("Required."),
		format!("{at}: description repeats what the generated bullet already prints"),
	);
	// The generated bullet prints an enum's accepted values from its variants().
	v.check(
		field.enum_variants.is_empty() || !prose.contains("Values:"),
		format!("{at}: description lists values the reference already renders from the type"),
	);
	// One spelling of a default, so a reader can find it by scanning.
	for wrong in ["(default", "Default:", "If not specified", "default is"] {
		v.check(
			!prose.contains(wrong),
			format!("{at}: default written as {wrong:?}; use \"Defaults to X.\""),
		);
	}
	v.check(
		field.is_required || prose.contains("Defaults to"),
		format!("{at}: optional parameter does not say what it defaults to"),
	);
	v.check(
		field.is_required || !prose.contains("Defaults to") || doc.ends_with('.'),
		format!("{at}: default sentence is malformed"),
	);
	for wrong in ["e.g.", "For example:", "i.e."] {
		v.check(
			!prose.contains(wrong),
			format!("{at}: uses {wrong:?}; write \"for example `k=v`\" instead"),
		);
	}
	// Literals belong in backticks, not in straight quotes.
	v.check(
		!prose.contains('"'),
		format!("{at}: quotes a literal; put it in backticks instead: {doc:?}"),
	);
	// A description is a label, not an explanation. Over the cap, the fix is
	// almost never shorter wording — it is moving the rationale half of the
	// sentence into the operation's details, where paragraphs render.
	v.check(
		doc.chars().count() <= MAX_DESCRIPTION_CHARS,
		format!(
			"{at}: description is {} characters, over the {MAX_DESCRIPTION_CHARS}-character cap; \
			 move the rationale into the operation's details: {doc:?}",
			doc.chars().count()
		),
	);
	// Conventions that hold for every parameter are stated once in `help.md`.
	for boilerplate in ["relative to the VPL", "relative to the vpl"] {
		v.check(
			!doc.contains(boilerplate),
			format!("{at}: repeats a convention already stated in help.md: {boilerplate:?}"),
		);
	}
}

fn tail(text: &str) -> String {
	text
		.chars()
		.rev()
		.take(40)
		.collect::<Vec<_>>()
		.into_iter()
		.rev()
		.collect()
}

#[test]
fn every_operation_follows_the_doc_style() {
	let mut v = Violations::default();
	let ops = all_operation_metadata();

	assert!(!ops.is_empty(), "no operations registered");

	for op in &ops {
		check_summary(&op.tag_name, op.summary.trim(), &mut v);
		check_details(&op.tag_name, op.details.trim(), &mut v);
		for field in &op.fields {
			check_field(&op.tag_name, field, &mut v);
		}
	}

	assert!(
		v.0.is_empty(),
		"{} operation doc-style violations (see versatiles_pipeline/DOCSTYLE.md):\n  {}",
		v.0.len(),
		v.0.join("\n  ")
	);
}

#[test]
fn prose_of_removes_backticked_syntax() {
	assert_eq!(prose_of("a `b=\"c\"` d"), "a d");
	assert_eq!(prose_of("no spans here"), "no spans here");
	assert_eq!(prose_of("`only`"), "");
	assert_eq!(prose_of("x `2^z - 1 - x` y"), "x y");
}

#[test]
fn has_collapsed_list_separates_bullets_from_hyphens() {
	assert!(has_collapsed_list("- first item"));
	assert!(has_collapsed_list("depends on the strategy: - min_distance: 16"));
	assert!(has_collapsed_list("Defaults to 0.5. - none: ignored."));
	assert!(!has_collapsed_list("a well - spaced hyphen"));
	assert!(!has_collapsed_list("an em dash — like this"));
	assert!(!has_collapsed_list(
		"Whether to mirror horizontally. Defaults to false."
	));
}
