#![cfg(feature = "cli")]
#![allow(clippy::float_cmp)]

mod test_utilities;
use pretty_assertions::assert_eq;
use test_utilities::*;
use versatiles_core::json::JsonValue;

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
		&input,
		output.to_str().unwrap()
	));

	assert!(output.exists(), "output file was not created: {output:?}");
	assert_eq!(
		tilejson(&output),
		JsonValue::parse_str(
			"{\"author\":\"OpenStreetMap contributors, Geofabrik GmbH\",\"bounds\":[13.08283,52.33446,13.762245,52.6783],\"center\":[13.425293,52.502477,2],\"description\":\"Tile config for simple vector tiles schema\",\"license\":\"Open Database License 1.0\",\"maxzoom\":14,\"minzoom\":0,\"name\":\"Tilemaker to Geofabrik Vector Tiles schema\",\"tilejson\":\"3.0.0\",\"type\":\"baselayer\",\"vector_layers\":[{\"fields\":{\"name\":\"String\",\"number\":\"String\"},\"id\":\"addresses\",\"maxzoom\":14,\"minzoom\":14},{\"fields\":{\"kind\":\"String\"},\"id\":\"aerialways\",\"maxzoom\":14,\"minzoom\":12},{\"fields\":{\"admin_level\":\"Number\",\"maritime\":\"Boolean\"},\"id\":\"boundaries\",\"maxzoom\":14,\"minzoom\":0},{\"fields\":{\"admin_level\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\",\"way_area\":\"Number\"},\"id\":\"boundary_labels\",\"maxzoom\":14,\"minzoom\":2},{\"fields\":{\"dummy\":\"Number\"},\"id\":\"buildings\",\"maxzoom\":14,\"minzoom\":14},{\"fields\":{\"kind\":\"String\"},\"id\":\"land\",\"maxzoom\":14,\"minzoom\":7},{\"fields\":{},\"id\":\"ocean\",\"maxzoom\":14,\"minzoom\":8},{\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\",\"population\":\"Number\"},\"id\":\"place_labels\",\"maxzoom\":14,\"minzoom\":3},{\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\"},\"id\":\"public_transport\",\"maxzoom\":14,\"minzoom\":11},{\"fields\":{\"kind\":\"String\"},\"id\":\"sites\",\"maxzoom\":14,\"minzoom\":14},{\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\",\"ref\":\"String\",\"ref_cols\":\"Number\",\"ref_rows\":\"Number\",\"tunnel\":\"Boolean\"},\"id\":\"street_labels\",\"maxzoom\":14,\"minzoom\":10},{\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\",\"ref\":\"String\"},\"id\":\"street_labels_points\",\"maxzoom\":14,\"minzoom\":12},{\"fields\":{\"bridge\":\"Boolean\",\"kind\":\"String\",\"rail\":\"Boolean\",\"service\":\"String\",\"surface\":\"String\",\"tunnel\":\"Boolean\"},\"id\":\"street_polygons\",\"maxzoom\":14,\"minzoom\":14},{\"fields\":{\"bicycle\":\"String\",\"bridge\":\"Boolean\",\"horse\":\"String\",\"kind\":\"String\",\"link\":\"Boolean\",\"rail\":\"Boolean\",\"service\":\"String\",\"surface\":\"String\",\"tracktype\":\"String\",\"tunnel\":\"Boolean\"},\"id\":\"streets\",\"maxzoom\":14,\"minzoom\":14},{\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\"},\"id\":\"streets_polygons_labels\",\"maxzoom\":14,\"minzoom\":14},{\"fields\":{\"kind\":\"String\"},\"id\":\"water_lines\",\"maxzoom\":14,\"minzoom\":4},{\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\"},\"id\":\"water_lines_labels\",\"maxzoom\":14,\"minzoom\":4},{\"fields\":{\"kind\":\"String\"},\"id\":\"water_polygons\",\"maxzoom\":14,\"minzoom\":4},{\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\"},\"id\":\"water_polygons_labels\",\"maxzoom\":14,\"minzoom\":14}],\"version\":\"3.0\"}"
		).unwrap()
	);
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
	assert_eq!(
		tilejson(&output),
		JsonValue::parse_str(
			"{\"author\":\"OpenStreetMap contributors, Geofabrik GmbH\",\"bounds\":[13.08283,52.33446,13.762245,52.6783],\"description\":\"Tile config for simple vector tiles schema\",\"format\":\"pbf\",\"license\":\"Open Database License 1.0\",\"maxzoom\":14,\"minzoom\":0,\"name\":\"Tilemaker to Geofabrik Vector Tiles schema\",\"tile_format\":\"vnd.mapbox-vector-tile\",\"tile_schema\":\"other\",\"tile_type\":\"vector\",\"tilejson\":\"3.0.0\",\"type\":\"baselayer\",\"vector_layers\":[{\"fields\":{\"name\":\"String\",\"number\":\"String\"},\"id\":\"addresses\",\"maxzoom\":14,\"minzoom\":14},{\"fields\":{\"kind\":\"String\"},\"id\":\"aerialways\",\"maxzoom\":14,\"minzoom\":12},{\"fields\":{\"admin_level\":\"Number\",\"maritime\":\"Boolean\"},\"id\":\"boundaries\",\"maxzoom\":14,\"minzoom\":0},{\"fields\":{\"admin_level\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\",\"way_area\":\"Number\"},\"id\":\"boundary_labels\",\"maxzoom\":14,\"minzoom\":2},{\"fields\":{\"dummy\":\"Number\"},\"id\":\"buildings\",\"maxzoom\":14,\"minzoom\":14},{\"fields\":{\"kind\":\"String\"},\"id\":\"land\",\"maxzoom\":14,\"minzoom\":7},{\"fields\":{},\"id\":\"ocean\",\"maxzoom\":14,\"minzoom\":8},{\"fields\":{\"city_id\":\"automatically added field\",\"city_population\":\"automatically added field\",\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\",\"population\":\"Number\"},\"id\":\"place_labels\",\"maxzoom\":14,\"minzoom\":3},{\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\"},\"id\":\"public_transport\",\"maxzoom\":14,\"minzoom\":11},{\"fields\":{\"kind\":\"String\"},\"id\":\"sites\",\"maxzoom\":14,\"minzoom\":14},{\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\",\"ref\":\"String\",\"ref_cols\":\"Number\",\"ref_rows\":\"Number\",\"tunnel\":\"Boolean\"},\"id\":\"street_labels\",\"maxzoom\":14,\"minzoom\":10},{\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\",\"ref\":\"String\"},\"id\":\"street_labels_points\",\"maxzoom\":14,\"minzoom\":12},{\"fields\":{\"bridge\":\"Boolean\",\"kind\":\"String\",\"rail\":\"Boolean\",\"service\":\"String\",\"surface\":\"String\",\"tunnel\":\"Boolean\"},\"id\":\"street_polygons\",\"maxzoom\":14,\"minzoom\":14},{\"fields\":{\"bicycle\":\"String\",\"bridge\":\"Boolean\",\"horse\":\"String\",\"kind\":\"String\",\"link\":\"Boolean\",\"rail\":\"Boolean\",\"service\":\"String\",\"surface\":\"String\",\"tracktype\":\"String\",\"tunnel\":\"Boolean\"},\"id\":\"streets\",\"maxzoom\":14,\"minzoom\":14},{\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\"},\"id\":\"streets_polygons_labels\",\"maxzoom\":14,\"minzoom\":14},{\"fields\":{\"kind\":\"String\"},\"id\":\"water_lines\",\"maxzoom\":14,\"minzoom\":4},{\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\"},\"id\":\"water_lines_labels\",\"maxzoom\":14,\"minzoom\":4},{\"fields\":{\"kind\":\"String\"},\"id\":\"water_polygons\",\"maxzoom\":14,\"minzoom\":4},{\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\"},\"id\":\"water_polygons_labels\",\"maxzoom\":14,\"minzoom\":14}],\"version\":\"3.0\"}"
		).unwrap()
	);
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
		&input,
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
		&input,
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
		&input,
		output.to_str().unwrap()
	));

	assert_eq!(tilejson_bounds(&output), [13.3, 52.45, 13.46, 52.55]);
}

#[test]
fn e2e_convert_no_bbox() {
	let input = get_testdata("berlin.mbtiles");
	let (_temp_dir, output) = get_temp_output("no_bbox.mbtiles");

	versatiles_run(&format!("convert {} {}", &input, output.to_str().unwrap()));

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
