//! Static validation of a VPL pipeline, without building it.
//!
//! Building a pipeline opens files and hits the network, so it cannot run on a
//! keystroke — and it conflates two different failures. `from_container
//! filename=missing.mbtiles` is the *world* being wrong; `from_debug
//! format=png nonsense=1` is the *pipeline* being wrong, and an editor should
//! be able to say so before anything is opened.
//!
//! [`PipelineFactory::check`](crate::PipelineFactory::check) reports the second
//! kind only. It performs no I/O.
//!
//! Values are checked as far as the metadata allows: a parameter with an
//! enumerated type is handed to that type's own parser
//! (its `TryFrom<&str>` implementation), so
//! `format=notaformat` is reported and the alias `format=pbf` is not. A value
//! that is merely the wrong *format* for an unenumerated type — `color=red` is
//! not hex — still needs building.

use std::collections::HashMap;

use crate::{
	PipelineFactory,
	operations::{read_operation_factories, transform_operation_factories},
	vpl::{VPLFieldMeta, VPLNode, VPLPipeline},
};

/// Parameter metadata for every operation of one kind, by name.
type Registry = HashMap<String, Vec<VPLFieldMeta>>;

/// Checks a pipeline against the built-in operations without building it.
///
/// The free-standing counterpart to [`PipelineFactory::check`], for callers that
/// have no factory — an editor, a CI check, a `--dry-run`. It needs no runtime
/// and no reader callback, so it cannot perform I/O even by accident.
#[must_use]
pub fn check_pipeline(pipeline: &VPLPipeline) -> Vec<VplProblem> {
	let read = read_operation_factories()
		.iter()
		.map(|f| (f.tag_name().to_string(), f.field_metadata()))
		.collect::<Registry>();
	let transform = transform_operation_factories()
		.iter()
		.map(|f| (f.tag_name().to_string(), f.field_metadata()))
		.collect::<Registry>();

	check_against(pipeline, &read, &transform)
}

fn check_against(pipeline: &VPLPipeline, read: &Registry, transform: &Registry) -> Vec<VplProblem> {
	let mut problems = Vec::new();
	check_pipeline_at(pipeline, read, transform, &mut Vec::new(), &mut problems);
	problems
}

/// Something wrong with a pipeline, found without building it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VplProblem {
	/// Human-readable description, phrased for the person who wrote the VPL.
	pub message: String,
	/// Which node, as a path of indices from the root pipeline.
	///
	/// `[2]` is the third node of the pipeline. Descending into a node's
	/// bracketed sources appends the source index and then the node index, so
	/// `[0, 1, 2]` is the third node of the second source of the first node.
	/// The same walk applies to a [`CstPipeline`](crate::vpl::CstPipeline), which
	/// is how a caller turns this back into a span.
	pub path: Vec<usize>,
	/// The parameter at fault, when the problem is about one.
	///
	/// Pairs with [`CstNode::property`](crate::vpl::CstNode::property) to locate
	/// the exact token.
	pub property: Option<String>,
}

impl VplProblem {
	fn new(path: &[usize], message: impl Into<String>) -> Self {
		VplProblem {
			message: message.into(),
			path: path.to_vec(),
			property: None,
		}
	}

	fn about(path: &[usize], property: &str, message: impl Into<String>) -> Self {
		VplProblem {
			message: message.into(),
			path: path.to_vec(),
			property: Some(property.to_string()),
		}
	}
}

impl PipelineFactory {
	/// Checks a pipeline against the operations registered on *this* factory,
	/// without building it.
	///
	/// Reports everything it can find rather than stopping at the first problem,
	/// so an editor can underline all of them at once.
	///
	/// What it decides about a parameter is its *shape*: how many values it
	/// takes, whether each one parses as the field's numeric type, and — for
	/// types with a `TryFrom<&str>` parser — whether that parser accepts it.
	/// Meaning the Rust type does not carry is out of reach. Closing that means
	/// giving the field a type that parses (#257, #260), the way `color` is a
	/// `HexColor` rather than a `String` and `crs` is an `EpsgCode` rather than
	/// a `u32` — not giving `check` a second opinion about values.
	///
	/// A type reaches as far as its own parser and no further. `EpsgCode` knows
	/// the register's range, so `epsg=99999` is refused here; whether `epsg=9999`
	/// names a real CRS is a question for `proj.db`, which means I/O, which this
	/// does not do.
	///
	/// So an empty result means nothing is wrong *with the pipeline* — building
	/// it can still fail because a file is missing, or because a value has the
	/// right shape and the wrong meaning.
	///
	/// Never performs I/O. Use [`check_pipeline`] when there is no factory to
	/// hand and the built-in operations are enough.
	#[must_use]
	pub fn check(&self, pipeline: &VPLPipeline) -> Vec<VplProblem> {
		check_against(pipeline, &self.read_registry(), &self.transform_registry())
	}
}

fn check_pipeline_at(
	pipeline: &VPLPipeline,
	read: &Registry,
	transform: &Registry,
	path: &mut Vec<usize>,
	problems: &mut Vec<VplProblem>,
) {
	if pipeline.pipeline.is_empty() {
		problems.push(VplProblem::new(path, "pipeline is empty"));
		return;
	}

	for (index, node) in pipeline.pipeline.iter().enumerate() {
		path.push(index);
		// The head reads; everything after it transforms.
		check_node(node, index == 0, read, transform, path, problems);
		path.pop();
	}
}

fn check_node(
	node: &VPLNode,
	is_head: bool,
	read: &Registry,
	transform: &Registry,
	path: &mut Vec<usize>,
	problems: &mut Vec<VplProblem>,
) {
	let name = node.name.as_str();
	let is_read = read.contains_key(name);
	let is_transform = transform.contains_key(name);

	// Position first: an operation used in the wrong place is a different
	// mistake from one that does not exist, and saying so is more useful than
	// "unknown operation".
	let fields = match (is_head, is_read, is_transform) {
		(true, true, _) => read.get(name),
		(false, _, true) => transform.get(name),
		(true, false, true) => {
			problems.push(VplProblem::new(
				path,
				format!("'{name}' is a transform operation, but a pipeline has to start with a read operation"),
			));
			None
		}
		(false, true, false) => {
			problems.push(VplProblem::new(
				path,
				format!("'{name}' is a read operation, so it can only be the first node of a pipeline"),
			));
			None
		}
		_ => {
			problems.push(VplProblem::new(path, format!("unknown operation '{name}'")));
			None
		}
	};

	if let Some(fields) = fields {
		check_properties(node, fields, path, problems);
		check_sources(node, fields, path, problems);
	}

	// Nested pipelines are checked whether or not the parent was understood.
	for (source_index, source) in node.sources.iter().enumerate() {
		path.push(source_index);
		check_pipeline_at(source, read, transform, path, problems);
		path.pop();
	}
}

fn check_properties(node: &VPLNode, fields: &[VPLFieldMeta], path: &[usize], problems: &mut Vec<VplProblem>) {
	for (key, values) in &node.properties {
		let Some(field) = fields.iter().find(|f| &f.name == key) else {
			let known = fields
				.iter()
				.filter(|f| !f.is_sources)
				.map(|f| f.name.as_str())
				.collect::<Vec<_>>();
			problems.push(VplProblem::about(
				path,
				key,
				format!(
					"'{}' has no parameter '{key}'. Available: {}",
					node.name,
					known.join(", ")
				),
			));
			continue;
		};

		// What counts as a problem is the derive's business, not this
		// function's: it emits probes that call the same parsers the accessors
		// call, so anything reported here is something building would refuse.
		// All that is left to do is name the operation the problem belongs to.
		let Some(validate) = field.validate else {
			continue;
		};
		for reason in validate(values) {
			problems.push(VplProblem::about(path, key, format!("'{}' {reason}", node.name)));
		}
	}

	for field in fields {
		if field.is_required && !field.is_sources && !node.properties.contains_key(&field.name) {
			problems.push(VplProblem::about(
				path,
				&field.name,
				format!("'{}' requires the parameter '{}'", node.name, field.name),
			));
		}
	}
}

fn check_sources(node: &VPLNode, fields: &[VPLFieldMeta], path: &[usize], problems: &mut Vec<VplProblem>) {
	let Some(sources) = fields.iter().find(|f| f.is_sources) else {
		if !node.sources.is_empty() {
			problems.push(VplProblem::new(path, format!("'{}' does not take sources", node.name)));
		}
		return;
	};

	if sources.is_required && node.sources.is_empty() {
		problems.push(VplProblem::new(
			path,
			format!("'{}' needs at least one source", node.name),
		));
	}
}

#[cfg(test)]
mod tests {
	use anyhow::Result;

	use crate::{PipelineFactory, vpl::parse_vpl};

	fn problems(vpl: &str) -> Vec<String> {
		let factory = PipelineFactory::new_dummy();
		factory
			.check(&parse_vpl(vpl).unwrap())
			.into_iter()
			.map(|p| p.message)
			.collect()
	}

	#[test]
	fn a_valid_pipeline_has_no_problems() {
		assert!(problems("from_debug format=png | filter level_max=5").is_empty());
		assert!(problems("from_stacked [ from_debug format=png, from_debug format=png ]").is_empty());
	}

	#[test]
	fn unknown_operation() {
		assert_eq!(problems("from_nonsense"), ["unknown operation 'from_nonsense'"]);
	}

	#[test]
	fn unknown_parameter() {
		let found = problems("from_debug format=png nonsense=1");
		assert_eq!(found.len(), 1, "{found:?}");
		assert!(
			found[0].starts_with("'from_debug' has no parameter 'nonsense'. Available: "),
			"{found:?}"
		);
	}

	#[test]
	fn missing_required_parameter() {
		assert_eq!(
			problems("from_container"),
			["'from_container' requires the parameter 'filename'"]
		);
	}

	#[test]
	fn invalid_enum_values_are_reported() {
		let found = problems("from_debug format=nonsense");
		assert_eq!(found.len(), 1, "{found:?}");
		assert!(
			found[0].starts_with("'from_debug' does not accept 'format=nonsense'. Values: "),
			"{found:?}"
		);
	}

	/// The reason this is checked with the parser rather than against
	/// `enum_variants`: the list is the canonical names, and the parsers take
	/// aliases besides. Reporting one would reject VPL that builds, which is
	/// worse than reporting nothing.
	#[test]
	fn accepted_aliases_are_never_reported() {
		assert!(problems("from_debug format=pbf").is_empty());
		assert!(problems("from_debug format=jpeg").is_empty());
		// The parsers normalise case and surrounding space, so `check` does too.
		assert!(problems("from_debug format=PNG").is_empty());
	}

	/// `MaxTileBytes` parses through `TryFrom<&str>` without a closed variant
	/// list, so it is checked but has no values to suggest.
	#[test]
	fn a_type_with_no_variant_list_is_still_checked() {
		assert_eq!(
			problems("from_geo filename=a.geojson max_tile_bytes=fnord"),
			[
				"'from_geo' does not accept 'max_tile_bytes=fnord': invalid max_tile_bytes 'fnord'; \
				 expected a byte count (e.g. 2097152) or 'none' to disable the cap"
			]
		);
		assert!(problems("from_geo filename=a.geojson max_tile_bytes=none").is_empty());
		assert!(problems("from_geo filename=a.geojson max_tile_bytes=4096").is_empty());
	}

	/// Every value of a list is judged, not just the first.
	///
	/// `from_grid.offset` is the last parameter that is a bare array of numbers
	/// rather than a type that parses as a group — this test and the one below
	/// have already moved twice as `color` and then `center` grew types. If it
	/// grows one too, these need a fixture rather than a real parameter.
	#[test]
	fn each_value_of_a_repeated_parameter_is_checked() {
		let found = problems("from_grid epsg=3035 size=1 bbox=[0,0,1,1] offset=[a,b]");
		assert_eq!(found.len(), 2, "{found:?}");
		assert!(found[1].contains("'offset=b'"), "{found:?}");
	}

	/// A scalar parameter given a list is wrong twice over, and both are worth
	/// saying: the value that does not parse, and the count. `get_property`
	/// refuses more than one value, so the builder rejects this either way.
	#[test]
	fn a_bad_value_and_a_bad_count_are_both_reported() {
		let found = problems("from_debug format=[png, nonsense]");
		assert_eq!(found.len(), 2, "{found:?}");
		assert!(
			found[0].starts_with("'from_debug' does not accept 'format=nonsense'."),
			"{found:?}"
		);
		assert_eq!(found[1], "'from_debug' expects a single value for 'format', got 2");
	}

	/// The shape of a value is decidable from the metadata even where its
	/// meaning is not: `bbox_border` is checked as a number without anyone
	/// claiming to know which borders are sensible.
	#[test]
	fn a_value_that_is_not_a_number_is_reported() {
		assert_eq!(
			problems("from_debug format=png | filter bbox_border=abc"),
			["'filter' does not accept 'bbox_border=abc': invalid digit found in string"]
		);
	}

	/// The parser's own error, not a guess at one: a border of four billion is
	/// a perfectly good number and a bad `u32`, and only `parse` knows which of
	/// those matters.
	#[test]
	fn a_number_out_of_range_is_reported() {
		assert_eq!(
			problems("from_debug format=png | filter bbox_border=4294967296"),
			["'filter' does not accept 'bbox_border=4294967296': number too large to fit in target type"]
		);
	}

	/// What a `u8` could not say. Seventeen parameters advertised `0..=255` for
	/// something that means `0..=30`, so `level_max=99` parsed fine and failed
	/// later, somewhere else (#260). The range is the type's now, and the same
	/// parser answers `check`.
	#[test]
	fn a_zoom_level_outside_the_pyramid_is_reported() {
		assert_eq!(
			problems("from_debug format=png | filter level_max=99"),
			["'filter' does not accept 'level_max=99': level (99) must be <= 30"]
		);
		assert_eq!(
			problems("from_debug format=png | filter level_max=abc"),
			["'filter' does not accept 'level_max=abc': zoom level 'abc' is not a number between 0 and 30"]
		);
		for vpl in [
			"from_debug format=png | filter level_min=0 level_max=30",
			"from_geo filename=a.geojson max_zoom=14",
		] {
			assert!(problems(vpl).is_empty(), "check rejected {vpl:?}: {:?}", problems(vpl));
		}
	}

	#[test]
	fn an_array_of_the_wrong_length_is_reported() {
		assert_eq!(
			problems("from_grid epsg=3035 size=1 bbox=[0,0,1,1] offset=[0,0,0]"),
			["'from_grid' expects 2 values for 'offset', got 3"]
		);
	}

	/// The arguments whose parsers used to run only when a dataset was opened:
	/// the two spellings of a placement, the band list and the nodata groups.
	/// An editor showed nothing for any of them, and the failure arrived
	/// minutes into a run (#260).
	///
	/// Behind the feature that registers the operations, like the operations
	/// themselves — `check` cannot report a parameter of an operation this
	/// build does not have.
	#[cfg(feature = "gdal")]
	#[test]
	fn a_gdal_argument_that_cannot_parse_is_reported() {
		assert_eq!(
			problems("from_gdal_raster filename=a.tif crs=0"),
			["'from_gdal_raster' does not accept 'crs=0': EPSG codes run from 1024 to 32766, so 0 is not one"]
		);
		assert_eq!(
			problems("from_gdal_raster filename=a.tif bounds=\"0,0,1\""),
			[
				"'from_gdal_raster' does not accept 'bounds=0,0,1': `bounds` needs 4 comma-separated numbers, but '0,0,1' has 3"
			]
		);
		assert_eq!(
			problems("from_gdal_raster filename=a.tif bounds=\"10,0,0,10\""),
			["'from_gdal_raster' does not accept 'bounds=10,0,0,10': `bounds` needs east > west, but got west=10, east=0"]
		);
		assert_eq!(
			problems("from_gdal_dem filename=a.tif geo_transform=\"0,0,0,1,0,-1\""),
			[
				"'from_gdal_dem' does not accept 'geo_transform=0,0,0,1,0,-1': `geo_transform` needs a non-zero pixel \
				 width as its 2nd number, but got 0"
			]
		);
		assert_eq!(
			problems("from_gdal_raster filename=a.tif bands=\"1,two,3\""),
			["'from_gdal_raster' does not accept 'bands=1,two,3': 'two' is not a band index"]
		);
		assert_eq!(
			problems("from_gdal_raster filename=a.tif bands=\"0,1,2\""),
			["'from_gdal_raster' does not accept 'bands=0,1,2': band indices are 1-based, so 0 is not one"]
		);
		assert_eq!(
			problems("from_gdal_raster filename=a.tif nodata=\"0,0,nothing\""),
			["'from_gdal_raster' does not accept 'nodata=0,0,nothing': 'nothing' is not a nodata value"]
		);

		for vpl in [
			"from_gdal_raster filename=a.tif bounds=\"0,0,10,10\"",
			"from_gdal_raster filename=a.tif geo_transform=\"400000,9.6,2.5,5610000,2.5,-9.6\"",
			"from_gdal_raster filename=a.tif bands=\"1,2,3\" nodata=\"0,0,0;255,255,255\"",
		] {
			assert!(problems(vpl).is_empty(), "check rejected {vpl:?}: {:?}", problems(vpl));
		}
	}

	/// Where the data is, as something a consumer can read — and, for the
	/// thirteen that take a path, refused early instead of on `to_path_buf`
	/// after the pipeline was built (#260).
	#[test]
	fn a_location_of_the_wrong_kind_is_reported() {
		assert_eq!(
			problems("from_csv filename=\"https://example.org/cities.csv\" lon_column=x lat_column=y"),
			["'from_csv' does not accept 'filename=https://example.org/cities.csv': \
				 'https://example.org/cities.csv' is a URL, and this argument takes a path to a file"]
		);
		assert_eq!(
			problems("from_tilejson url=\"tiles/index.json\""),
			[
				"'from_tilejson' does not accept 'url=tiles/index.json': 'tiles/index.json' is not a URL: relative URL without a base"
			]
		);

		for vpl in [
			// A container may be a URL, a path, or standard input.
			"from_container filename=\"https://example.org/world.versatiles\"",
			"from_container filename=\"world.versatiles\"",
			"from_container filename=\"-\"",
			"from_csv filename=\"cities.csv\" lon_column=x lat_column=y",
			"from_tilejson url=\"https://example.org/tiles.json\"",
		] {
			assert!(problems(vpl).is_empty(), "check rejected {vpl:?}: {:?}", problems(vpl));
		}
	}

	/// The three JSON documents, refused before anything is built — including
	/// the layer structure, not just the JSON syntax (#260).
	///
	/// Only the inline halves: `tilejson_file` and its siblings name a file,
	/// and reading one is I/O that `check` does not do.
	#[test]
	fn a_json_document_that_does_not_parse_is_reported() {
		let found = problems("from_debug format=mvt | meta_update tilejson='{not valid'");
		assert_eq!(found.len(), 1, "{found:?}");
		assert!(found[0].contains("while parsing JSON string"), "{found:?}");

		let found = problems("from_debug format=mvt | meta_update vector_layers='[{\"fields\":{}}]'");
		assert_eq!(found.len(), 1, "{found:?}");
		assert!(found[0].contains("missing `id`"), "a layer without an id: {found:?}");

		let found = problems("from_debug format=mvt | meta_update vector_layers='{\"id\":\"roads\"}'");
		assert_eq!(found.len(), 1, "{found:?}");
		assert!(
			found[0].contains("array"),
			"an object where an array belongs: {found:?}"
		);

		for vpl in [
			"from_debug format=mvt | meta_update tilejson='{\"tilejson\":\"3.0.0\"}'",
			"from_debug format=mvt | meta_update vector_layers='[{\"id\":\"roads\",\"fields\":{}}]'",
			// The file halves are still paths, and still nobody's business here.
			"from_debug format=mvt | meta_update tilejson_file='not-read-by-check.json'",
		] {
			assert!(problems(vpl).is_empty(), "check rejected {vpl:?}: {:?}", problems(vpl));
		}
	}

	/// Two languages whose compilers only ever ran when the operation was
	/// built: a regex and a CEL expression. An editor showed nothing for either
	/// (#260). The CEL parser panics on some malformed input rather than
	/// returning an error, which `compile_cel` contains — so `check` can be
	/// asked about a half-typed expression without taking the caller down.
	#[test]
	fn a_pattern_or_expression_that_does_not_compile_is_reported() {
		let found = problems(r#"from_debug format=mvt | vector_filter_properties regex="[unclosed""#);
		assert_eq!(found.len(), 1, "{found:?}");
		assert!(
			found[0].starts_with("'vector_filter_properties' does not accept 'regex=[unclosed'"),
			"{found:?}"
		);

		let found = problems(r#"from_debug format=mvt | vector_filter_features layer=["a"] expr="foo @@""#);
		assert_eq!(found.len(), 1, "{found:?}");
		assert!(found[0].contains("Failed to compile CEL expression"), "{found:?}");

		// The input that panics the parser rather than erroring.
		let found = problems(r#"from_debug format=mvt | vector_filter_features layer=["a"] expr="population >=""#);
		assert_eq!(found.len(), 1, "{found:?}");
		assert!(found[0].contains("Parser crashed"), "{found:?}");

		for vpl in [
			r#"from_debug format=mvt | vector_filter_properties regex="^name:""#,
			r#"from_debug format=mvt | vector_filter_features layer=["a"] expr="props.population > 1000""#,
		] {
			assert!(problems(vpl).is_empty(), "check rejected {vpl:?}: {:?}", problems(vpl));
		}
	}

	/// The H3 resolution, which was a `u8` with its range in an `if` inside
	/// `from_args` — so `resolution=99` built far enough to ask H3 about it.
	#[test]
	fn an_h3_resolution_outside_the_grid_is_reported() {
		assert_eq!(
			problems("from_h3 resolution=99"),
			["'from_h3' does not accept 'resolution=99': H3Resolution must be between 0 and 15, but is 99"]
		);
		assert!(problems("from_h3 resolution=7").is_empty());
	}

	/// The three bounded numbers whose bound lived in a doc comment and nowhere
	/// else: `gamma=-5` and `effort=200` both *built*, and produced whatever
	/// they produced. Two of them are "above `0`", which is why the type
	/// carries an exclusive bound rather than a min/max pair (#260).
	#[test]
	fn a_number_outside_its_documented_bound_is_reported() {
		assert_eq!(
			problems("from_debug format=png | raster_levels gamma=-5"),
			["'raster_levels' does not accept 'gamma=-5': Gamma must be greater than 0.0, but is -5"]
		);
		assert_eq!(
			problems("from_debug format=png | raster_levels contrast=0"),
			["'raster_levels' does not accept 'contrast=0': Contrast must be greater than 0.0, but is 0"]
		);
		assert_eq!(
			problems("from_debug format=png | raster_levels brightness=300"),
			[
				"'raster_levels' does not accept 'brightness=300': Brightness must be between -255.0 and 255.0, but \
				 is 300"
			]
		);
		assert_eq!(
			problems("from_debug format=png | raster_format format=png effort=200"),
			["'raster_format' does not accept 'effort=200': Effort must be between 0 and 100, but is 200"]
		);

		for vpl in [
			"from_debug format=png | raster_levels gamma=0.5 contrast=1.2 brightness=-20",
			"from_debug format=png | raster_format format=png effort=100",
		] {
			assert!(problems(vpl).is_empty(), "check rejected {vpl:?}: {:?}", problems(vpl));
		}
	}

	/// The last of the four Tier 1 gaps for `center`: three numbers that were
	/// only ever counted, never read. `GeoCenter::check` existed the whole time
	/// — a longitude of 1000 reached the TileJSON that clients read because
	/// nothing on the way asked it (#260).
	#[test]
	fn a_centre_off_the_globe_is_reported() {
		assert_eq!(
			problems("from_debug format=png | meta_update center=[1000,99,7]"),
			["'meta_update' does not accept 'center=[1000,99,7]': center[0] (longitude) must be <= 180"]
		);
		assert_eq!(
			problems("from_debug format=png | meta_update center=[13.4,52.5,99]"),
			["'meta_update' does not accept 'center=[13.4,52.5,99]': center[2] (zoom) must be <= 30"]
		);
		assert!(problems("from_debug format=png | meta_update center=[13.4,52.5,12]").is_empty());
	}

	/// The case #255 is about, with the corners transposed rather than the CRS
	/// wrong — and unlike the CRS, decidable from the literal alone.
	///
	/// A `GeoBBox` is judged as a group because that is the only way to see it:
	/// every one of these four numbers is a fine `f64` on its own, and it is the
	/// relationship between them that is wrong.
	#[test]
	fn a_bounding_box_is_judged_as_a_group() {
		assert_eq!(
			problems("from_debug format=png | filter bbox=[13.5,52.6,13.3,52.4]"),
			["'filter' does not accept 'bbox=[13.5,52.6,13.3,52.4]': x_min (13.5) must be <= x_max (13.3)"]
		);
		assert_eq!(
			problems("from_debug format=png | filter bbox=[0,0,200,10]"),
			["'filter' does not accept 'bbox=[0,0,200,10]': x_max (200) must be <= 180"]
		);
		assert_eq!(
			problems("from_debug format=png | filter bbox=[0,0,10]"),
			["'filter' does not accept 'bbox=[0,0,10]': expected 4 values [west, south, east, north], got 3"]
		);
		assert!(problems("from_debug format=png | filter bbox=[13.3,52.4,13.5,52.6]").is_empty());
	}

	/// A list-typed parameter takes any number of values and judges none of
	/// them, so it carries no validator at all.
	#[test]
	fn a_string_list_takes_any_number_of_values() {
		assert!(problems("from_debug format=png | vector_filter_features layer=[a,b,c] expr=true").is_empty());
	}

	#[test]
	fn composite_without_sources() {
		assert_eq!(problems("from_stacked"), ["'from_stacked' needs at least one source"]);
	}

	#[test]
	fn operation_in_the_wrong_position() {
		assert_eq!(
			problems("filter level_max=5"),
			["'filter' is a transform operation, but a pipeline has to start with a read operation"]
		);
		assert_eq!(
			problems("from_debug format=png | from_debug format=png"),
			["'from_debug' is a read operation, so it can only be the first node of a pipeline"]
		);
	}

	#[test]
	fn problems_are_reported_for_nested_pipelines_with_a_path() {
		let factory = PipelineFactory::new_dummy();
		let found = factory
			.check(&parse_vpl("from_stacked [ from_debug format=png, from_debug nonsense=1 ]").unwrap())
			.into_iter()
			.map(|p| (p.path, p.property))
			.collect::<Vec<_>>();

		// First node of the root, its second source, that source's first node.
		assert_eq!(found, [(vec![0, 1, 0], Some("nonsense".to_string()))]);
	}

	/// A file extension the operation cannot read is a problem `check` reports,
	/// not one the build discovers.
	///
	/// This is what typing `from_geo.filename` as a `GeoDataPath` bought beyond
	/// the dialog filter: the derive emits the type's `TryFrom` into
	/// `VPLFieldMeta::validate`, so an editor underlines `notes.txt` as it is
	/// typed. As a `FilePath` the value parsed fine here and failed later, in
	/// `from_geo`'s own dispatch.
	#[test]
	fn an_extension_the_operation_cannot_read_is_reported_without_building() {
		let found = problems("from_geo filename=\"notes.txt\"");
		assert_eq!(found.len(), 1, "expected one problem, got {found:?}");
		assert!(
			found[0].contains("unsupported file extension"),
			"expected the extension to be named, got: {}",
			found[0]
		);

		assert!(
			problems("from_geo filename=\"places.geojson\"").is_empty(),
			"an extension from_geo reads is not a problem"
		);
	}

	#[test]
	fn every_problem_is_reported_not_just_the_first() {
		assert_eq!(problems("from_debug nonsense=1 | filter rubbish=2").len(), 2);
	}

	/// The rule this whole feature exists to keep: `check` must not reject
	/// anything the builder accepts. Studio maintained this as a differential
	/// test against `operation_from_vpl` because there was no API; it belongs
	/// here.
	#[tokio::test]
	async fn check_never_rejects_a_pipeline_the_builder_accepts() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		for vpl in [
			"from_debug format=png",
			"from_debug format=pbf",
			"from_debug format=png | filter level_min=2 level_max=5",
			"from_debug format=png | filter bbox=[0,0,10,10] bbox_border=2",
			"from_stacked [ from_debug format=png, from_debug format=png ]",
			"from_debug format=png | raster_format format=webp",
			"from_color color=FF5733",
			"from_color",
		] {
			assert!(
				factory.build_pipeline(parse_vpl(vpl)?).await.is_ok(),
				"precondition: the builder should accept {vpl:?}"
			);
			assert!(
				factory.check(&parse_vpl(vpl)?).is_empty(),
				"check rejected a pipeline the builder accepts: {vpl:?} -> {:?}",
				factory.check(&parse_vpl(vpl)?)
			);
		}
		Ok(())
	}

	/// The converse, for the cases the issue lists: anything `check` rejects,
	/// the builder must reject too — otherwise `--dry-run` would refuse work
	/// that would have succeeded.
	#[tokio::test]
	async fn the_builder_rejects_everything_check_rejects() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		for vpl in [
			"from_nonsense",
			"from_debug format=png nonsense=1",
			"from_container",
			"from_stacked",
			"filter level_max=5",
			"from_debug format=notaformat",
			"from_debug format=[png, png]",
			"from_debug format=png | filter level_max=abc",
			"from_debug format=png | filter level_max=300",
			"from_debug format=png | raster_flatten color=[0,0]",
			"from_debug format=png | filter bbox=[13.5,52.6,13.3,52.4]",
			"from_debug format=png | filter bbox=[0,0,10]",
			"from_color color=red",
			"from_debug format=png | remap_coords flip_x=yess",
			"from_debug format=png | raster_format quality=110",
		] {
			assert!(!factory.check(&parse_vpl(vpl)?).is_empty(), "check accepted {vpl:?}");
			assert!(
				factory.build_pipeline(parse_vpl(vpl)?).await.is_err(),
				"the builder accepted something check rejects: {vpl:?}"
			);
		}
		Ok(())
	}

	/// A typo in a flag used to mean the opposite of what it said: anything that
	/// was not `1/true/yes/ok` parsed as `false`, silently. Now it is refused,
	/// by the same function the accessor calls.
	#[test]
	fn a_value_that_is_not_a_boolean_is_reported() {
		assert_eq!(
			problems("from_debug format=png | remap_coords flip_x=yess"),
			["'remap_coords' does not accept 'flip_x=yess': expected a boolean (1/true/yes/ok or 0/false/no), got 'yess'"]
		);
		for spelling in ["1", "true", "YES", " ok ", "0", "false", "no"] {
			let vpl = format!("from_debug format=png | remap_coords flip_x=\"{spelling}\"");
			assert!(problems(&vpl).is_empty(), "{spelling:?} should be a boolean");
		}
	}

	/// The gap #257 was opened for, closed for the field it was opened about.
	/// `color` is a `HexColor`, so the parser that turns `RRGGBB` into bytes is
	/// the one `check` asks — not a second opinion written to agree with it.
	#[test]
	fn a_value_in_the_wrong_format_is_reported() {
		assert_eq!(
			problems("from_color color=red"),
			["'from_color' does not accept 'color=[red]': Invalid hex color 'red': invalid digit found in string"]
		);
		assert!(problems("from_color color=FF5733").is_empty());
		assert!(problems("from_color color=FF573380").is_empty());
	}

	/// The tightening #260 asked for. `from_gdal_raster` and `from_gdal_dem`
	/// took any `u32` and documented only "Tile size in pixels", so
	/// `tile_size=1024` built a pyramid no client asks for; the other three
	/// refused it, but only once something was built. One type now answers for
	/// all five, and `check` answers without building anything.
	#[test]
	fn a_tile_size_outside_the_closed_set_is_reported() {
		assert_eq!(
			problems("from_color tile_size=300"),
			["'from_color' does not accept 'tile_size=300'. Values: 256, 512"]
		);
		assert_eq!(
			problems("from_debug format=png | raster_tile_resize tile_size=1024"),
			["'raster_tile_resize' does not accept 'tile_size=1024'. Values: 256, 512"]
		);
		for vpl in ["from_color tile_size=256", "from_color tile_size=512"] {
			assert!(problems(vpl).is_empty(), "check rejected {vpl:?}");
		}
	}

	/// A colour has two spellings and both are accepted, so `raster_flatten`
	/// keeps the `r,g,b` it took before it had a type while gaining the hex
	/// `from_color` always had (#260). VPL splits on commas, so the two arrive
	/// as different value counts and only a group parser can tell them apart.
	#[test]
	fn both_spellings_of_a_colour_are_accepted() {
		for vpl in [
			"from_color color=FF5733",
			"from_color color=[255,87,51]",
			"from_debug format=png | raster_flatten color=FF5733",
			"from_debug format=png | raster_flatten color=[255,255,255]",
		] {
			assert!(problems(vpl).is_empty(), "check rejected {vpl:?}: {:?}", problems(vpl));
		}
	}

	/// Neither spelling: too few numbers to be channels, too many values to be
	/// hex. The count belongs to the colour's own parser now, which is why the
	/// message names both spellings instead of demanding three values.
	#[test]
	fn a_colour_that_is_neither_spelling_is_reported() {
		assert_eq!(
			problems("from_debug format=png | raster_flatten color=[0,0]"),
			[
				"'raster_flatten' does not accept 'color=[0,0]': a colour is one hex value like `RRGGBB`, \
				 or 3 to 4 channel numbers, but 2 values were given"
			]
		);
	}

	/// The tail of #257: three parameters whose format lived in a doc comment and
	/// an `if` inside `build`, now in a type each. Nothing here is a correctness
	/// bug — they are typo-catchers — but the typo used to cost however long it
	/// took to open the source first.
	#[test]
	fn formats_that_used_to_be_checked_only_at_build_time() {
		assert_eq!(
			problems("from_debug format=png | raster_format quality=110"),
			[
				"'raster_format' does not accept 'quality=110': Parsing quality string: quality must be between 0 and 100, but is 110"
			]
		);
		assert_eq!(
			problems("from_csv filename=a.csv lon_column=x lat_column=y delimiter=\";;\""),
			[
				r#"'from_csv' does not accept 'delimiter=;;': separator must be one character or an escape (\t, \n, \r), got ";;""#
			]
		);
		assert_eq!(
			problems(
				"from_debug format=mvt | vector_update_properties data_source_path=a.csv layer_name=l id_field_tiles=a id_field_data=b field_separator=\";;\""
			),
			[
				r#"'vector_update_properties' does not accept 'field_separator=;;': separator must be one character or an escape (\t, \n, \r), got ";;""#
			]
		);

		assert!(problems("from_debug format=png | raster_format quality=\"70,14:50,15:20\"").is_empty());
		assert!(problems("from_csv filename=a.csv lon_column=x lat_column=y delimiter=\";\"").is_empty());
	}

	/// Every way of writing a tab means a tab, for both parameters. It used to
	/// depend on which operation you were writing and which quotes you used:
	/// VPL resolves escapes inside double quotes but not single ones, so `'\t'`
	/// arrives as two characters — a tab for `field_separator`, an error for
	/// `delimiter`, with a message that printed `\t` back and looked like it was
	/// denying its own rule.
	#[test]
	fn a_tab_is_a_tab_however_it_is_spelled() {
		for tab in ["\"\\t\"", "'\\t'", "\"\\\\t\""] {
			assert!(
				problems(&format!(
					"from_csv filename=a.csv lon_column=x lat_column=y delimiter={tab}"
				))
				.is_empty(),
				"delimiter={tab}"
			);
			assert!(
				problems(&format!(
					"from_debug format=mvt | vector_update_properties data_source_path=a.csv \
					 layer_name=l id_field_tiles=a id_field_data=b field_separator={tab}"
				))
				.is_empty(),
				"field_separator={tab}"
			);
		}
	}

	/// The gap this test used to hold open, closed — and the one still left.
	///
	/// `epsg` was the example of a meaning no type carried; it is an `EpsgCode`
	/// now, so a code outside the register is refused without building anything
	/// (#260). How far that reaches is the type's range and no further: `9999`
	/// is inside the register's range and is not a CRS, and only `proj.db` knows
	/// that — which is I/O, which `check` does not do.
	#[test]
	fn a_code_outside_the_epsg_register_is_reported() {
		assert_eq!(
			problems("from_grid epsg=99999 size=1 bbox=[0,0,1,1]"),
			["'from_grid' does not accept 'epsg=99999': EPSG codes run from 1024 to 32766, so 99999 is not one"]
		);
		for vpl in [
			"from_grid epsg=3035 size=1 bbox=[0,0,1,1]",
			// In range, not in the register: shallower than GDAL, on purpose.
			"from_grid epsg=9999 size=1 bbox=[0,0,1,1]",
		] {
			assert!(problems(vpl).is_empty(), "check rejected {vpl:?}: {:?}", problems(vpl));
		}
	}
}
