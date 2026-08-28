//! Pins the VPL metadata surface — the contract consumers outside this repo read.
//!
//! `all_operation_metadata()` is a public API even where the Rust API is not.
//! versatiles-studio builds its pickers and forms from it: a field's
//! `rust_type` decides whether a value gets a map picker, a colour swatch or a
//! plain text box, and `enum_variants` is the list a dropdown offers. So
//! `[f64;4]` becoming `GeoBBox`, or `String` becoming `HexColor`, changes what
//! other people's software renders while changing nothing that a compiler
//! would complain about — and it fails in the plausible direction, turning a
//! working picker into a text box that still accepts typing.
//!
//! Renames and removals are the same story without the type change. 4.11 alone
//! renamed `from_color.size` to `tile_size` and dropped `max_zoom` from the
//! grid operations; both were invisible in the release notes, and a consumer
//! had to diff two registries to find them.
//!
//! This test makes that diff a reviewable one. Every change to the surface
//! shows up in `metadata_surface.snap` in the same commit that causes it, which
//! is the moment to decide whether it needs a release-note line.
//!
//! # What it does *not* cover
//!
//! Doc prose. `docs_style.rs` owns that, and folding it in here would churn the
//! snapshot on every wording fix — burying the surface changes this exists to
//! show.
//!
//! # Regenerating
//!
//! ```sh
//! UPDATE_METADATA_SNAPSHOT=1 cargo test -p versatiles_pipeline \
//!   --features codegen,gdal --test metadata_surface
//! ```
//!
//! Then read the diff before committing it. A line that moved is a rename; a
//! line that vanished is a removal; a changed `rust_type` is a rendering change
//! downstream.

#![cfg(feature = "codegen")]

use std::{collections::BTreeMap, fmt::Write as _, fs};

use versatiles_pipeline::{OperationMeta, all_operation_metadata, vpl::VPLFieldMeta};

/// Committed beside this file so a diff of the surface lands in the same review
/// as the change that caused it.
const SNAPSHOT_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/metadata_surface.snap");

/// Operations the registry only holds when a feature is on.
///
/// The snapshot is written from a run with every feature enabled, so a run
/// without one has to know which absences are legitimate. Listing them by name
/// rather than inferring them means a third gated operation cannot quietly join
/// the set: it would read as an unexplained removal, which is the whole point.
const FEATURE_GATED: &[(&str, &str)] = &[("from_gdal_raster", "gdal"), ("from_gdal_dem", "gdal")];

/// Whether the feature named in [`FEATURE_GATED`] is on for *this* build.
fn feature_is_on(feature: &str) -> bool {
	match feature {
		"gdal" => cfg!(feature = "gdal"),
		other => panic!("`{other}` is in FEATURE_GATED but nothing here can tell whether it is on"),
	}
}

/// One line per field, self-describing, so a diff hunk reads without context.
fn render_field(op: &str, field: &VPLFieldMeta) -> String {
	let mut line = format!(
		"{op}.{}: {} | {}",
		field.name,
		field.rust_type,
		if field.is_required { "required" } else { "optional" }
	);
	if field.is_sources {
		line.push_str(" | sources");
	}
	if let Some(default) = &field.default {
		let _ = write!(line, " | default={default}");
	}
	if !field.enum_variants.is_empty() {
		let _ = write!(line, " | values=[{}]", field.enum_variants.join(","));
	}
	line
}

/// The block for one operation: a header naming its kind, then its fields in
/// declaration order — the order the reference and the TypeScript bindings
/// render them in, so a reordering is a surface change worth seeing.
fn render_operation(op: &OperationMeta) -> String {
	let mut block = format!("{} [{}]\n", op.tag_name, op.kind);
	for field in &op.fields {
		block.push_str(&render_field(&op.tag_name, field));
		block.push('\n');
	}
	block
}

/// The whole surface, operations sorted by name.
///
/// Sorted because the registry's order is an implementation detail: shuffling
/// two `Box::new(…)` lines in `operations/mod.rs` is not a change to what
/// anyone can observe, and should not produce a diff here.
fn render(ops: &[OperationMeta]) -> String {
	let mut sorted: Vec<&OperationMeta> = ops.iter().collect();
	sorted.sort_by(|a, b| a.tag_name.cmp(&b.tag_name));

	let mut out = String::from(
		"# The VPL metadata surface, as consumers see it. Generated — see tests/metadata_surface.rs.\n\
		 # A diff in this file is a change other people's software can see.\n\n",
	);
	for op in sorted {
		out.push_str(&render_operation(op));
		out.push('\n');
	}
	out
}

/// Splits a rendered surface into one block per operation, keyed by name.
fn blocks(text: &str) -> BTreeMap<String, String> {
	text
		.split("\n\n")
		.filter(|block| !block.starts_with('#') && !block.trim().is_empty())
		.map(|block| {
			// Trimmed first: a hand-edited file can leave a stray blank line, and
			// an operation named "\nfilter" would report as a removal plus an
			// addition rather than as the whitespace it is.
			let block = block.trim();
			let name = block
				.split_once(" [")
				.unwrap_or_else(|| panic!("block does not start with an operation header:\n{block}"))
				.0;
			(name.to_string(), block.to_string())
		})
		.collect()
}

/// The first differing lines, as a diff a reader can act on.
///
/// `assert_eq!` on two multi-kilobyte strings prints both in full and leaves
/// the reader to find the change, which for a snapshot is no help at all.
fn describe_difference(expected: &str, actual: &str) -> Option<String> {
	if expected == actual {
		return None;
	}
	let old: Vec<&str> = expected.lines().collect();
	let new: Vec<&str> = actual.lines().collect();

	let mut lines = Vec::new();
	for line in &old {
		if !new.contains(line) {
			lines.push(format!("  - {line}"));
		}
	}
	for line in &new {
		if !old.contains(line) {
			lines.push(format!("  + {line}"));
		}
	}
	if lines.is_empty() {
		// Same lines, different order: a field or operation moved.
		lines.push("  (the same lines in a different order)".to_string());
	}
	Some(lines.join("\n"))
}

#[test]
fn the_metadata_surface_matches_the_committed_snapshot() {
	let ops = all_operation_metadata();
	assert!(!ops.is_empty(), "no operations registered");
	let rendered = render(&ops);

	if std::env::var_os("UPDATE_METADATA_SNAPSHOT").is_some() {
		// Regenerating from a partial build would silently delete the operations
		// this build cannot see — the exact failure the snapshot exists to catch.
		assert!(
			FEATURE_GATED.iter().all(|(_, feature)| feature_is_on(feature)),
			"regenerate with every feature on: --features codegen,gdal"
		);
		fs::write(SNAPSHOT_PATH, &rendered).expect("snapshot is writable");
		return;
	}

	let snapshot = fs::read_to_string(SNAPSHOT_PATH).expect("snapshot is readable");
	let expected = blocks(&snapshot);
	let actual = blocks(&rendered);

	// Absences this build can account for, and only those. An operation missing
	// for any other reason is a removal, and reads as one.
	let excused: Vec<&str> = FEATURE_GATED
		.iter()
		.filter(|(_, feature)| !feature_is_on(feature))
		.map(|(op, _)| *op)
		.collect();

	let missing: Vec<&String> = expected
		.keys()
		.filter(|name| !actual.contains_key(*name) && !excused.contains(&name.as_str()))
		.collect();
	assert!(
		missing.is_empty(),
		"operations in the snapshot but not in the registry: {missing:?}\n\
		 If they were removed on purpose, regenerate the snapshot — and say so in the release notes, \
		 because a consumer's form loses those fields."
	);

	let added: Vec<&String> = actual.keys().filter(|name| !expected.contains_key(*name)).collect();
	assert!(
		added.is_empty(),
		"operations in the registry but not in the snapshot: {added:?}\n\
		 Regenerate with UPDATE_METADATA_SNAPSHOT=1 (see tests/metadata_surface.rs)."
	);

	let mut differences = Vec::new();
	for (name, actual_block) in &actual {
		if let Some(diff) = describe_difference(&expected[name], actual_block) {
			differences.push(format!("{name}:\n{diff}"));
		}
	}
	assert!(
		differences.is_empty(),
		"the metadata surface changed in {} operation(s):\n\n{}\n\n\
		 A `rust_type` change alters what consumers render (a picker becomes a text box), and a \
		 rename removes a field they read by name. If the change is intended, regenerate with \
		 UPDATE_METADATA_SNAPSHOT=1 and give it a release-note line.",
		differences.len(),
		differences.join("\n\n")
	);
}

/// The snapshot only guards what it lists, so it has to list everything this
/// build can see — including the operations behind a feature.
///
/// Without this, running the suite with `gdal` off and regenerating would be a
/// silent way to delete two operations from the contract.
#[test]
fn every_registered_operation_is_in_the_snapshot() {
	if std::env::var_os("UPDATE_METADATA_SNAPSHOT").is_some() {
		// The sibling test is rewriting the file this one reads, and the two run
		// on separate threads. Nothing to check against a file mid-write.
		return;
	}

	let snapshot = fs::read_to_string(SNAPSHOT_PATH).expect("snapshot is readable");
	let listed = blocks(&snapshot);

	for (op, feature) in FEATURE_GATED {
		assert!(
			listed.contains_key(*op),
			"`{op}` is behind the `{feature}` feature and missing from the snapshot, so nothing \
			 guards its metadata. Regenerate with that feature on."
		);
	}

	let registered = all_operation_metadata().len() + FEATURE_GATED.iter().filter(|(_, f)| !feature_is_on(f)).count();
	assert_eq!(
		listed.len(),
		registered,
		"the snapshot lists {} operations, this build knows {registered}",
		listed.len()
	);
}
