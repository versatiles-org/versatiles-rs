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

use versatiles_pipeline::{
	OperationMeta, all_operation_metadata,
	vpl::{Accepts, Bounds, FieldReference, VPLFieldMeta},
};

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
	if let Some(bounds) = &field.bounds {
		let _ = write!(line, " | bounds={}", render_bounds(bounds));
	}
	if let Some(reference) = &field.refers_to {
		let _ = write!(line, " | names={}", render_reference(*reference));
	}
	if let Some(accepts) = &field.accepts {
		let _ = write!(line, " | accepts={}", render_accepts(*accepts));
	}
	line
}

/// Which files an argument takes. `only=` is a set a dialog can filter on,
/// `suggested=` one it should preselect and let be overridden, and `any` is a
/// file argument with no filter — distinct from a line with no `accepts=` at
/// all, which is an argument that does not name a file.
fn render_accepts(accepts: Accepts) -> String {
	match accepts {
		Accepts::Any => "any".to_string(),
		Accepts::Suggested(list) => format!("suggested[{}]", list.join(",")),
		Accepts::Only(list) => format!("only[{}]", list.join(",")),
	}
}

/// What an argument names in the data, when it names something there.
fn render_reference(reference: FieldReference) -> String {
	match reference {
		FieldReference::FieldOf { argument } => format!("field-of:{argument}"),
		FieldReference::FieldOfSource => "field-of-source".to_string(),
		FieldReference::LayerOfSource => "layer-of-source".to_string(),
		FieldReference::NewLayer => "new-layer".to_string(),
		FieldReference::NewField => "new-field".to_string(),
	}
}

/// A range in interval notation: square where an end is included, round where
/// it is not or where there is no end at all.
fn render_bounds(bounds: &Bounds) -> String {
	let open = if bounds.min.is_some() && bounds.min_inclusive {
		'['
	} else {
		'('
	};
	let close = if bounds.max.is_some() && bounds.max_inclusive {
		']'
	} else {
		')'
	};
	let low = bounds.min.map_or_else(|| "-inf".to_string(), |value| value.to_string());
	let high = bounds.max.map_or_else(|| "inf".to_string(), |value| value.to_string());
	let kind = if bounds.integer { "int" } else { "float" };
	format!("{open}{low},{high}{close} {kind}")
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

/// Every argument that names a file says which files.
///
/// The compile-time floor is in the `VPLDecode` derive, which refuses a path
/// argument that declares nothing. This is the runtime half: it survives a
/// refactor of the derive, and it is what fails if a path type is added to
/// `PATH_TYPES` without the guard reaching it.
///
/// The prose was always there — "GeoJSON polygon outside which pixels become
/// transparent", "Path to the CSV or TSV file" — and a consumer building a file
/// dialog could not read it. Fifteen arguments, of which a per-operation guess
/// got two actively wrong and eight not at all.
#[test]
fn every_argument_naming_a_file_says_which_files() {
	let ops = all_operation_metadata();
	let mut seen = 0;

	for op in &ops {
		for field in &op.fields {
			let names_a_file = field.rust_type.contains("FilePath")
				|| field.rust_type.contains("SourceLocation")
				|| field.rust_type.contains("GeoDataPath");
			if !names_a_file {
				continue;
			}
			seen += 1;
			assert!(
				field.accepts.is_some(),
				"{}.{} is typed {} but says nothing about which files it takes",
				op.tag_name,
				field.name,
				field.rust_type
			);
		}
	}

	// The floor: a rule keyed on a shape stops having a subject when the shape
	// is renamed, and would then pass by vacuum.
	assert!(seen >= 15, "expected the fifteen path arguments, found {seen}");
}

/// `Only` is the accepted set, so everything in it has to build.
///
/// The list `from_geo` publishes is the list its type refuses everything
/// outside of, which is the whole difference between this and the table that
/// was deleted for being aspirational: that one recorded `["geojson", …]` for
/// `cutline` and nothing ever consulted it. This consults it.
#[test]
fn what_from_geo_offers_is_what_from_geo_reads() {
	let ops = all_operation_metadata();
	let field = ops
		.iter()
		.find(|op| op.tag_name == "from_geo")
		.and_then(|op| op.fields.iter().find(|f| f.name == "filename"))
		.expect("from_geo has a filename");

	let Some(Accepts::Only(offered)) = field.accepts else {
		panic!("from_geo.filename refuses what it does not read, so it offers a closed set");
	};

	let validate = field.validate.expect("a typed path parameter validates its value");
	for ext in offered {
		let problems = validate(&[format!("data.{ext}")]);
		assert!(problems.is_empty(), "'.{ext}' is offered but rejected: {problems:?}");
	}
	assert!(
		!validate(&["data.txt".to_string()]).is_empty(),
		"'.txt' is not offered and must not be accepted"
	);
	assert!(
		!validate(&["data".to_string()]).is_empty(),
		"a file with no extension names no format"
	);
}

/// The same, for the tile formats `from_tile` reads.
///
/// `TileFilePath` delegates its parser to `TileFormat`, so this is what keeps
/// the *published* list in step with it: a format added to `TileFormat` and not
/// to `EXTENSIONS` shows up here rather than as a dialog quietly filtering out
/// a file that would have opened.
#[test]
fn what_from_tile_offers_is_what_tile_format_reads() {
	let ops = all_operation_metadata();
	let field = ops
		.iter()
		.find(|op| op.tag_name == "from_tile")
		.and_then(|op| op.fields.iter().find(|f| f.name == "filename"))
		.expect("from_tile has a filename");

	let Some(Accepts::Only(offered)) = field.accepts else {
		panic!("from_tile.filename offers the formats it reads");
	};

	let validate = field.validate.expect("a typed path parameter validates its value");
	for ext in offered {
		assert!(
			versatiles_core::TileFormat::try_from_str(ext).is_ok(),
			"'.{ext}' is offered but names no tile format"
		);
		let problems = validate(&[format!("tile.{ext}")]);
		assert!(problems.is_empty(), "'.{ext}' is offered but rejected: {problems:?}");
	}
	assert!(
		!validate(&["tile.txt".to_string()]).is_empty(),
		"'.txt' names no tile format and must not be accepted"
	);
}

/// `Any` is a filter of its own, not a missing one.
///
/// `ssh_identity` was handed its operation's container filter — `versatiles,
/// mbtiles, pmtiles, tar` — for a private key, which is conventionally
/// extensionless. The correct filter is none, and that has to be sayable.
#[test]
fn an_argument_whose_set_is_not_ours_to_state_says_so() {
	let ops = all_operation_metadata();
	for (op_name, field_name) in [
		("from_container", "ssh_identity"),
		("from_gdal_raster", "filename"),
		("from_gdal_dem", "filename"),
	] {
		let Some(op) = ops.iter().find(|op| op.tag_name == op_name) else {
			continue; // gdal operations are feature-gated
		};
		let field = op
			.fields
			.iter()
			.find(|f| f.name == field_name)
			.unwrap_or_else(|| panic!("{op_name} has a {field_name}"));
		assert_eq!(
			field.accepts,
			Some(Accepts::Any),
			"{op_name}.{field_name} takes any file; a list here would be a guess"
		);
	}
}

/// A parameter named like a zoom level is typed as one.
///
/// The snapshot pins the seventeen that exist; this is about the eighteenth.
/// Adding `level_max: Option<u8>` to a new operation would show up there as one
/// more plausible-looking line, and nothing would object — while `level_max=99`
/// went on parsing fine and failing later (#260). The naming convention is
/// already consistent, so it is worth holding to.
#[test]
fn every_parameter_named_like_a_zoom_level_is_typed_as_one() {
	let ops = all_operation_metadata();
	let mut seen = 0;

	for op in &ops {
		for field in &op.fields {
			let name = field.name.as_str();
			let names_a_zoom =
				name == "level" || name.starts_with("level_") || name.ends_with("_zoom") || name.ends_with("zoom");
			if !names_a_zoom {
				continue;
			}
			seen += 1;
			assert_eq!(
				field.rust_type,
				"Option<ZoomLevel>",
				"{}.{name} is named like a zoom level but typed {}; `u8` advertises 0..=255 for \
				 something that means 0..={}",
				op.tag_name,
				field.rust_type,
				versatiles_core::MAX_ZOOM_LEVEL
			);
		}
	}

	// The floor: a rule keyed on a shape stops having a subject when the shape
	// goes, and goes green rather than red. Thirteen without `gdal`; the two
	// GDAL read operations carry two zoom parameters each.
	let expected = if cfg!(feature = "gdal") { 17 } else { 13 };
	assert_eq!(
		seen, expected,
		"expected {expected} zoom-level parameters, found {seen} — did they stop sharing a name or a type?"
	);
}

/// Every zoom parameter describes the range it accepts, not just enforces it.
///
/// B3 typed these seventeen and, for a consumer, took something away: `u8` at
/// least said "whole numbers, 0 to 255" — wrong at the top end but readable —
/// while `ZoomLevel` said nothing at all until `bounds()` existed. A form that
/// wants to bound its input should not have to hardcode 30 (#260).
#[test]
fn every_zoom_parameter_describes_its_range() {
	let ops = all_operation_metadata();
	let mut seen = 0;

	for op in &ops {
		for field in op.fields.iter().filter(|f| f.rust_type == "Option<ZoomLevel>") {
			seen += 1;
			let bounds = field.bounds.expect("a zoom parameter describes its range");
			assert_eq!((bounds.min, bounds.max), (Some(0.0), Some(30.0)), "{}", op.tag_name);
			assert!(bounds.min_inclusive && bounds.max_inclusive);
			assert!(bounds.integer, "a zoom control steps by one");
		}
	}

	let expected = if cfg!(feature = "gdal") { 17 } else { 13 };
	assert_eq!(seen, expected, "expected {expected} zoom parameters, found {seen}");
}

/// A parameter with no domain type of its own still describes itself, from the
/// range of its Rust number type. Saying "whole numbers, 0 to 4294967295" is
/// less useful than a real bound and much more useful than silence — and it is
/// where `integer` comes from for every field that has no `bounds()`.
#[test]
fn a_plain_number_falls_back_to_the_range_of_its_rust_type() {
	let ops = all_operation_metadata();

	let border = find_field(&ops, "filter", "bbox_border")
		.bounds
		.expect("u32 has a range");
	assert_eq!((border.min, border.max), (Some(0.0), Some(4_294_967_295.0)));
	assert!(border.integer);

	// `gamma` stood here until it grew a type with a real bound of its own.
	let tolerance = find_field(&ops, "from_geo", "polygon_simplify")
		.bounds
		.expect("f32 is still describable");
	assert_eq!(
		(tolerance.min, tolerance.max),
		(None, None),
		"no end is not the same as no answer"
	);
	assert!(!tolerance.integer, "a form should allow decimals here");

	// Not a number, so there is nothing to describe.
	assert!(find_field(&ops, "from_container", "filename").bounds.is_none());
}

/// Look up `(op_name, field_name)` in the live metadata.
fn find_field(ops: &[OperationMeta], op_name: &str, field_name: &str) -> VPLFieldMeta {
	let op = ops
		.iter()
		.find(|o| o.tag_name == op_name)
		.unwrap_or_else(|| panic!("operation `{op_name}` not registered"));
	op.fields
		.iter()
		.find(|f| f.name == field_name)
		.unwrap_or_else(|| panic!("field `{field_name}` not found on `{op_name}`"))
		.clone()
}

/// Every argument that names something in the data says so, and says what.
///
/// The one thing on the surface no type expresses: `lon_column` names a column
/// of the file `filename` names, which is a relationship between two arguments
/// (#260). A consumer was left matching on argument names and reading doc
/// comments to rebuild it.
///
/// The derive already refuses a `field_of` pointing at an argument that does
/// not exist, so what is left to check here is that the annotations are still
/// *there* — a rule keyed on a shape goes green, not red, when the shape goes.
#[test]
fn every_argument_that_names_data_says_what_it_names() {
	let ops = all_operation_metadata();
	let mut counts: BTreeMap<String, usize> = BTreeMap::new();

	for op in &ops {
		for field in &op.fields {
			if let Some(reference) = field.refers_to {
				*counts.entry(render_reference(reference)).or_default() += 1;
			}
		}
	}

	// The counts today. Adding an annotated argument is fine; losing one is an
	// edit to make on purpose, because a form loses a picker with it.
	assert!(counts["field-of:filename"] >= 7, "{counts:?}");
	assert!(counts["field-of:data_source_path"] >= 1, "{counts:?}");
	assert!(counts["field-of-source"] >= 1, "{counts:?}");
	assert!(counts["layer-of-source"] >= 3, "{counts:?}");
	assert!(counts["new-layer"] >= 4, "{counts:?}");
	assert!(counts["new-field"] >= 4, "{counts:?}");

	// Spot-checks, so the counts cannot be satisfied by the wrong fields.
	assert_eq!(
		find_field(&ops, "from_csv", "lon_column").refers_to,
		Some(FieldReference::FieldOf { argument: "filename" })
	);
	assert_eq!(
		find_field(&ops, "vector_update_properties", "id_field_tiles").refers_to,
		Some(FieldReference::FieldOfSource)
	);
	assert_eq!(
		find_field(&ops, "from_csv", "filename").refers_to,
		None,
		"the file itself does not name a field of anything"
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
