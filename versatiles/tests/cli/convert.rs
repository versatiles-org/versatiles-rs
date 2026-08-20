use pretty_assertions::assert_eq;
use versatiles_core::json::JsonValue;

use crate::test_utilities::*;

#[test]
fn e2e_convert_requires_input_and_output() {
	let o = versatiles_output("convert");
	assert!(!o.success);
	assert_eq!(o.code, 2);
	assert!(o.stdout.is_empty());
	assert_contains!(
		&o.stderr,
		&format!("Usage: {BINARY_NAME} convert [OPTIONS] <INPUT_FILE> <OUTPUT_FILE>")
	);
}

#[test]
fn e2e_convert_mbtiles_to_versatiles() {
	let input = get_testdata("berlin.mbtiles");
	let (_temp_dir, output) = get_temp_output("berlin.versatiles");

	versatiles_run(&format!("convert {input} {}", output.to_str().unwrap()));

	assert!(output.exists(), "output file was not created: {output:?}");
}

#[test]
fn e2e_convert_pmtiles_to_mbtiles_with_bbox_and_border() {
	let input = get_testdata("berlin.pmtiles");
	let (_temp_dir, output) = get_temp_output("berlin-bbox.mbtiles");

	versatiles_run(&format!(
		"convert --bbox 13.0,52.0,13.8,52.8 --bbox-border 1 {} {}",
		input,
		output.to_str().unwrap()
	));

	assert!(output.exists(), "output file was not created: {output:?}");
	let tj = tilejson(&output);
	let obj = tj.as_object().expect("tilejson is object");
	assert_eq!(obj.get("name").and_then(|v| v.as_str().ok()), Some("VersaTiles OSM"));
	assert_eq!(obj.get("maxzoom").and_then(|v| v.as_number().ok()), Some(14.0));
	// The requested bbox is larger than the fixture (~[13.3, 52.45, 13.46,
	// 52.55]), so the crop clamps to the fixture; `--bbox-border 1` then extends
	// the bounds outward to the tile-aligned ring actually written (wider on the
	// south/north edges here).
	assert_eq!(tilejson_bounds(&output), [13.3, 52.449314, 13.46, 52.556316]);
	// Verify the Shortbread layer set survived the round-trip.
	let layer_ids = vector_layer_ids(&tj);
	assert!(layer_ids.contains(&"land".to_string()));
	assert!(layer_ids.contains(&"streets".to_string()));
	assert!(layer_ids.contains(&"buildings".to_string()));
}

/// Regression: with a bbox *smaller* than the source, `--bbox-border` writes a
/// ring of tiles around the crop. The advertised bounds must grow to include
/// that ring, otherwise a client honoring tilejson bounds (e.g. MapLibre) would
/// never request the border tiles that exist in the container.
#[test]
fn e2e_convert_bbox_border_widens_bounds() {
	let input = get_testdata("berlin.pmtiles");

	// Crop well inside the fixture (~[13.3, 52.45, 13.46, 52.55]).
	let crop = [13.35, 52.48, 13.40, 52.52];

	let convert = |border: u32, name: &str| -> [f64; 4] {
		let (_temp_dir, output) = get_temp_output(name);
		versatiles_run(&format!(
			"convert --bbox {},{},{},{} --bbox-border {border} {input} {}",
			crop[0],
			crop[1],
			crop[2],
			crop[3],
			output.to_str().unwrap()
		));
		tilejson_bounds(&output)
	};

	let plain = convert(0, "crop-plain.mbtiles");
	let bordered = convert(3, "crop-border.mbtiles");

	// The border must push every side outward beyond the plain crop.
	assert!(bordered[0] < plain[0], "west must widen: {bordered:?} vs {plain:?}");
	assert!(bordered[1] < plain[1], "south must widen: {bordered:?} vs {plain:?}");
	assert!(bordered[2] > plain[2], "east must widen: {bordered:?} vs {plain:?}");
	assert!(bordered[3] > plain[3], "north must widen: {bordered:?} vs {plain:?}");

	// ...but it stays a small regional box around the crop, not the whole world.
	assert!(bordered[0] > 13.0 && bordered[2] < 13.8, "stays regional: {bordered:?}");
	assert!(bordered[1] > 52.2 && bordered[3] < 52.8, "stays regional: {bordered:?}");
}

#[test]
fn e2e_convert_vpl_via_stdin() {
	let testdata_pmtiles = get_testdata("berlin.pmtiles");
	let testdata_csv = get_testdata("cities.csv");
	let stdin = format!(
		r#"
			from_container filename='{testdata_pmtiles}' |
			vector_update_properties
				data_source_path='{testdata_csv}'
				layer_name="place_labels"
				id_field_tiles="name"
				id_field_data="city_name"
		"#
	)
	.replace('\t', "   ");

	let (_temp_dir, output) = get_temp_output("vpl.pmtiles");
	versatiles_stdin(&format!("convert [,vpl]- {}", output.to_str().unwrap()), &stdin);

	assert!(output.exists(), "output file was not created: {output:?}");
	let tj = tilejson(&output);
	// The whole point of this test: `vector_update_properties` should have
	// added the `city_id` and `city_population` fields to the place_labels
	// layer (sourced from cities.csv via name <-> city_name match).
	let place_labels = vector_layer_by_id(&tj, "place_labels").expect("place_labels layer present");
	let fields = place_labels
		.as_object()
		.expect("layer is object")
		.get("fields")
		.expect("layer has fields")
		.as_object()
		.expect("fields is object");
	assert!(fields.get("city_id").is_some(), "city_id not added: {fields:?}");
	assert!(
		fields.get("city_population").is_some(),
		"city_population not added: {fields:?}"
	);
}

/// Extracts the list of `vector_layers[].id` strings from a parsed tilejson.
fn vector_layer_ids(tj: &JsonValue) -> Vec<String> {
	let layers = tj
		.as_object()
		.expect("tilejson is object")
		.get("vector_layers")
		.expect("has vector_layers")
		.as_array()
		.expect("vector_layers is array");
	layers
		.iter()
		.filter_map(|l| l.as_object().ok()?.get("id")?.as_str().ok().map(str::to_string))
		.collect()
}

/// Looks up a vector_layer entry by its `id` string.
fn vector_layer_by_id<'a>(tj: &'a JsonValue, id: &str) -> Option<&'a JsonValue> {
	let layers = tj.as_object().ok()?.get("vector_layers")?.as_array().ok()?;
	layers.iter().find(|l| {
		l.as_object()
			.ok()
			.and_then(|o| o.get("id"))
			.and_then(|v| v.as_str().ok())
			== Some(id)
	})
}

// --- Convert command bbox tests ---
//
// All assertions below are computed against the fixture's actual bounds
// ([13.3, 52.45, 13.46, 52.55] for the small repaired-from-osm.versatiles
// berlin fixture).

#[test]
fn e2e_convert_bbox_inside_source() {
	let input = get_testdata("berlin.mbtiles");
	let (_temp_dir, output) = get_temp_output("bbox_inside.mbtiles");

	// A bbox strictly inside the fixture's bounds → output bounds = arg.
	versatiles_run(&format!(
		"convert --bbox 13.35,52.48,13.42,52.52 {} {}",
		input,
		output.to_str().unwrap()
	));

	assert_eq!(tilejson_bounds(&output), [13.35, 52.48, 13.42, 52.52]);
}

#[test]
fn e2e_convert_bbox_partial_overlap() {
	let input = get_testdata("berlin.mbtiles");
	let (_temp_dir, output) = get_temp_output("bbox_partial.mbtiles");

	// arg overlaps the eastern half of the fixture → clamped intersection.
	versatiles_run(&format!(
		"convert --bbox 13.4,52.5,13.7,52.6 {} {}",
		input,
		output.to_str().unwrap()
	));

	assert_eq!(tilejson_bounds(&output), [13.4, 52.5, 13.46, 52.55]);
}

#[test]
fn e2e_convert_bbox_contains_source() {
	let input = get_testdata("berlin.mbtiles");
	let (_temp_dir, output) = get_temp_output("bbox_contains.mbtiles");

	// arg wider than fixture in every direction → output keeps fixture bounds.
	versatiles_run(&format!(
		"convert --bbox 12.0,51.0,15.0,54.0 {} {}",
		input,
		output.to_str().unwrap()
	));

	assert_eq!(tilejson_bounds(&output), [13.3, 52.45, 13.46, 52.55]);
}

#[test]
fn e2e_convert_no_bbox() {
	let input = get_testdata("berlin.mbtiles");
	let (_temp_dir, output) = get_temp_output("no_bbox.mbtiles");

	versatiles_run(&format!("convert {} {}", input, output.to_str().unwrap()));

	assert_eq!(tilejson_bounds(&output), [13.3, 52.45, 13.46, 52.55]);
}

// --- VPL filter bbox tests ---

#[test]
fn e2e_vpl_filter_bbox() {
	let input = get_testdata("berlin.mbtiles");
	// filter bbox strictly inside fixture → result = filter bbox.
	let vpl = format!("from_container filename='{input}' | filter bbox=[13.35,52.48,13.42,52.52]");

	let (_temp_dir, bounds) = get_bounds_from_vpl(&vpl);
	assert_eq!(bounds, [13.35, 52.48, 13.42, 52.52]);
}

#[test]
fn e2e_vpl_chained_filters() {
	let input = get_testdata("berlin.mbtiles");
	// outer filter wider than inner; inner inside fixture → result = inner.
	let vpl = format!(
		"from_container filename='{input}' | filter bbox=[13.0,52.0,13.5,52.6] | filter bbox=[13.35,52.48,13.42,52.52]"
	);

	let (_temp_dir, bounds) = get_bounds_from_vpl(&vpl);
	assert_eq!(bounds, [13.35, 52.48, 13.42, 52.52]);
}

// --- Combination operation bbox tests ---

#[test]
fn e2e_vpl_from_merged_vector_bbox() {
	let input = get_testdata("berlin.mbtiles");
	// Two non-overlapping sub-bboxes inside the fixture → merged bounds = union.
	let vpl = format!(
		"from_merged_vector [\n\
		   from_container filename='{input}' | filter bbox=[13.30,52.45,13.37,52.55],\n\
		   from_container filename='{input}' | filter bbox=[13.40,52.45,13.46,52.55]\n\
		]"
	);

	let (_temp_dir, bounds) = get_bounds_from_vpl(&vpl);
	assert_eq!(bounds, [13.30, 52.45, 13.46, 52.55]);
}

#[test]
fn e2e_vpl_from_stacked_bbox() {
	let input = get_testdata("berlin.mbtiles");
	let vpl = format!(
		"from_stacked [\n\
		   from_container filename='{input}' | filter bbox=[13.30,52.45,13.37,52.55],\n\
		   from_container filename='{input}' | filter bbox=[13.40,52.45,13.46,52.55]\n\
		]"
	);

	let (_temp_dir, bounds) = get_bounds_from_vpl(&vpl);
	assert_eq!(bounds, [13.30, 52.45, 13.46, 52.55]);
}

#[test]
fn e2e_vpl_from_stacked_raster_bbox() {
	let vpl = "from_stacked_raster [\n\
		   from_debug format=png | filter bbox=[10.0,50.0,12.0,52.0] level_max=4,\n\
		   from_debug format=png | filter bbox=[14.0,54.0,16.0,56.0] level_max=4\n\
		]"
	.to_string();

	let (_temp_dir, bounds) = get_bounds_from_vpl(&vpl);
	assert_eq!(bounds, [10.0, 50.0, 16.0, 56.0]);
}
