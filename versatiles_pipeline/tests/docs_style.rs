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

/// The literal a "Defaults to `X`." sentence promises, if it promises one.
///
/// A default is stated in exactly one form — `docs_style` enforces that much
/// already — so the literal is whatever sits in backticks immediately after
/// "Defaults to " and immediately before the closing period. Anything else is a
/// *computed* default ("Defaults to the source's highest.", "Defaults to a
/// heuristic capped at `14`.", "Defaults to `16`/`0.5`."), which has no literal
/// to promise and yields `None`.
fn stated_default(doc: &str) -> Option<&str> {
	let start = doc.rfind("Defaults to ")? + "Defaults to ".len();
	let tail = doc[start..].strip_prefix('`')?.strip_suffix('.')?;
	let literal = tail.strip_suffix('`')?;
	// One backticked span, not two: "Defaults to `16`/`0.5`." promises neither.
	(!literal.contains('`')).then_some(literal)
}

/// Whether a stated default is a value at all, rather than a reference to
/// something else.
///
/// `filter`'s `level_min` defaults to `level_max` — the backticks are naming
/// another parameter, not a value to put in a form. `raster_overview`'s `level_max` defaults to
/// `level_base + 4`, an expression rather than a literal. Neither is something
/// `VPLFieldMeta::default` should carry, so neither is required to.
fn is_a_value(literal: &str, siblings: &[String]) -> bool {
	!literal.contains(char::is_whitespace) && !siblings.iter().any(|name| name == literal)
}

/// Holds `#[vpl(default = "…")]` against the doc comment a reader actually
/// sees, in both directions.
///
/// The attribute is what a generated form shows and the sentence is what the
/// reference prints, so they are one fact written twice — and a form promising
/// a default that changed two releases ago is exactly the failure this is meant
/// to prevent.
fn check_default(op: &str, field: &VPLFieldMeta, siblings: &[String], v: &mut Violations) {
	let at = format!("{op}.{}", field.name);
	let stated = stated_default(field.doc.trim());

	match (&field.default, stated) {
		(Some(declared), Some(literal)) => v.check(
			declared == literal,
			format!("{at}: declares the default {declared:?} but its description says {literal:?}"),
		),
		(Some(declared), None) => v.check(
			false,
			format!(
				"{at}: declares the default {declared:?}, which its description does not state; \
				 write \"Defaults to `{declared}`.\""
			),
		),
		(None, Some(literal)) => v.check(
			!is_a_value(literal, siblings),
			format!(
				"{at}: describes the default `{literal}` but does not declare it; add #[vpl(default = \"{literal}\")]"
			),
		),
		(None, None) => (),
	}
}

/// A type with a closed set of values already says what they are; a
/// description that lists them too is one fact written twice, and the two
/// drift apart.
///
/// They did: five `tile_size` parameters documented one range three ways —
/// three said "`256` or `512`", two said only "Tile size in pixels." — so a
/// reader assembling the set from the prose got three right and could not tell
/// which two they had missed. Typing the field ended that (#260); this keeps it
/// ended.
///
/// The default sentence is exempt. "Defaults to `512`." names one value as a
/// value, not as the range, and `check_default` already holds it against the
/// declared default.
fn check_states_no_variants(op: &str, field: &VPLFieldMeta, v: &mut Violations) {
	if field.enum_variants.len() < 2 {
		return;
	}
	let doc = field.doc.trim();
	let without_default = doc.rfind("Defaults to ").map_or(doc, |at| &doc[..at]);

	let named = field
		.enum_variants
		.iter()
		.filter(|variant| without_default.contains(**variant))
		.copied()
		.collect::<Vec<_>>();

	v.check(
		named.len() < 2,
		format!(
			"{op}.{}: its description lists {named:?}, which its type already carries; \
			 say what the value is and let the type say which ones there are",
			field.name
		),
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

	let mut closed_sets = 0;
	for op in &ops {
		check_summary(&op.tag_name, op.summary.trim(), &mut v);
		check_details(&op.tag_name, op.details.trim(), &mut v);
		let names = op.fields.iter().map(|f| f.name.clone()).collect::<Vec<_>>();
		for field in &op.fields {
			check_field(&op.tag_name, field, &mut v);
			if !field.is_sources {
				check_default(&op.tag_name, field, &names, &mut v);
			}
			if field.enum_variants.len() >= 2 {
				closed_sets += 1;
				check_states_no_variants(&op.tag_name, field, &mut v);
			}
		}
	}

	// A rule of the form "every field shaped like X must be Y" stops having a
	// subject when X does, and goes green rather than red. Typing fields is
	// exactly what empties such a rule, so the count it examined is asserted
	// too. Adding one is fine; losing one is an edit to make on purpose.
	//
	// Feature-dependent, because the registry is: the two GDAL read operations
	// carry four closed-set parameters between them, and a workspace build
	// unifies `codegen` on without `gdal`.
	let expected = if cfg!(feature = "gdal") { 20 } else { 16 };
	assert!(
		closed_sets >= expected,
		"only {closed_sets} parameters carry a closed set of values, expected {expected}; the variant \
		 rule has almost nothing left to check, which is a change to look at rather than to accept"
	);

	assert!(
		v.0.is_empty(),
		"{} operation doc-style violations (see versatiles_pipeline/DOCSTYLE.md):\n  {}",
		v.0.len(),
		v.0.join("\n  ")
	);
}

#[test]
fn stated_default_finds_only_a_bare_literal() {
	assert_eq!(stated_default("Tile size in pixels. Defaults to `512`."), Some("512"));
	assert_eq!(stated_default("Whether to climb. Defaults to `false`."), Some("false"));
	assert_eq!(stated_default("Lower-left corner. Defaults to `[0,0]`."), Some("[0,0]"));
	// Computed, so there is no literal to promise.
	assert_eq!(stated_default("Zoom level. Defaults to the source's highest."), None);
	assert_eq!(
		stated_default("Highest zoom. Defaults to a heuristic capped at `14`."),
		None
	);
	assert_eq!(stated_default("Distance. Defaults to `16`/`0.5`."), None);
	assert_eq!(stated_default("Filename of the CSV file."), None);
}

#[test]
fn a_reference_to_another_parameter_is_not_a_value() {
	let siblings = ["level_min".to_string(), "level_max".to_string()];
	assert!(!is_a_value("level_max", &siblings));
	assert!(!is_a_value("level_base + 4", &siblings));
	assert!(is_a_value("512", &siblings));
	assert!(is_a_value("[0,0]", &siblings));
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
