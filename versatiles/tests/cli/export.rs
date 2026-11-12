use crate::test_utilities::*;

/// The tile `convert` writes into a directory, uncompressed: the bytes
/// `export tile` has to agree with.
fn tile_via_convert(input: &str) -> Vec<u8> {
	let (_dir, output) = get_temp_output("tiles");
	versatiles_run(&format!(
		"convert -q --min-zoom 14 --max-zoom 14 --bbox 13.40,52.52,13.40,52.52 -c uncompressed {input} {}",
		output.to_str().unwrap()
	));
	std::fs::read(output.join("14/8801/5373.pbf")).unwrap()
}

#[test]
fn e2e_export_tile_writes_the_decompressed_tile() {
	let input = get_testdata("berlin.mbtiles");
	let (_dir, output) = get_temp_output("tile.pbf");

	versatiles_run(&format!(
		"export tile -q {input} 14/8801/5373 -o {}",
		output.to_str().unwrap()
	));

	assert_eq!(std::fs::read(&output).unwrap(), tile_via_convert(&input));
}

#[test]
fn e2e_export_tile_at_a_point_and_to_standard_output() {
	let input = get_testdata("berlin.mbtiles");
	let output = versatiles_cmd()
		.args(["export", "tile", "-q", &input, "--at", "13.40,52.52", "--zoom", "14"])
		.output()
		.unwrap();
	assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
	assert_eq!(output.stdout, tile_via_convert(&input));
}

#[test]
fn e2e_export_tile_raw_keeps_the_stored_compression() {
	let input = get_testdata("berlin.mbtiles");
	let (_dir, output) = get_temp_output("tile.pbf.gz");

	versatiles_run(&format!(
		"export tile -q --raw {input} 14/8801/5373 -o {}",
		output.to_str().unwrap()
	));

	let bytes = std::fs::read(&output).unwrap();
	assert_eq!(bytes[..2], [0x1f, 0x8b], "a gzip stream");
}

#[test]
fn e2e_export_tile_refuses_a_missing_tile_and_a_misleading_name() {
	let input = get_testdata("berlin.mbtiles");
	let (_dir, output) = get_temp_output("tile.pbf");
	let output = output.to_str().unwrap();

	let o = versatiles_output(&format!("export tile -q {input} 14/0/0 -o {output}"));
	assert!(!o.success);
	assert_contains!(&o.stderr, "has no tile 14/0/0");

	let o = versatiles_output(&format!("export tile -q {input} 14/8801/5373 -o {output}.gz"));
	assert!(!o.success);
	assert_contains!(&o.stderr, "or add --raw");
}

/// A raster source: `from_debug` draws every tile, 512 pixels wide.
fn debug_source(dir: &std::path::Path) -> String {
	let path = dir.join("debug.vpl");
	std::fs::write(&path, "from_debug format=png").unwrap();
	path.to_str().unwrap().to_string()
}

#[test]
fn e2e_export_area_writes_an_image_and_a_world_file() {
	let (dir, output) = get_temp_output("area.png");
	let input = debug_source(dir.path());

	// From the prime meridian to 90° east and from the equator northwards, at
	// zoom 2: exactly one tile, so 512 pixels square.
	versatiles_run(&format!(
		"export area -q {input} --bbox 0,0,90,66.5132604431 --zoom 2 -o {}",
		output.to_str().unwrap()
	));

	let image = image_size(&std::fs::read(&output).unwrap());
	assert_eq!(image, (512, 512));
	let world_file = std::fs::read_to_string(output.with_extension("pgw")).unwrap();
	let lines: Vec<f64> = world_file.lines().map(|line| line.parse().unwrap()).collect();
	assert_eq!(lines.len(), 6);
	let pixel = 2.0 * 20_037_508.342_789_244 / 2048.0;
	assert!((lines[0] - pixel).abs() < 1e-6, "{world_file}");
	assert!((lines[4] - pixel / 2.0).abs() < 1e-3, "{world_file}");
}

/// Width and height from a PNG header.
fn image_size(png: &[u8]) -> (u32, u32) {
	let read = |at: usize| u32::from_be_bytes(png[at..at + 4].try_into().unwrap());
	(read(16), read(20))
}

#[test]
fn e2e_export_area_refuses_what_it_cannot_write() {
	let (dir, output) = get_temp_output("area.png");
	let input = debug_source(dir.path());
	let output = output.to_str().unwrap();

	let o = versatiles_output(&format!(
		"export area -q {input} --bbox -10,-10,10,10 --zoom 10 --max-pixels 1000 -o {output}"
	));
	assert!(!o.success);
	assert_contains!(&o.stderr, "more than --max-pixels 1000");

	let o = versatiles_output(&format!(
		"export area -q {input} --bbox -10,-10,10,10 --zoom 2 -o area.tif"
	));
	assert!(!o.success);
	assert_contains!(&o.stderr, "has to end in .png, .webp or .jpg");

	let mbtiles = get_testdata("berlin.mbtiles");
	let o = versatiles_output(&format!(
		"export area -q {mbtiles} --bbox 13.3,52.4,13.5,52.6 --zoom 14 -o {output}"
	));
	assert!(!o.success);
	assert_contains!(&o.stderr, "holds mvt tiles");
}

/// Parses GeoJSON and returns its features.
fn features(geojson: &str) -> Vec<versatiles_core::json::JsonValue> {
	let json = versatiles_core::json::parse_json_str(geojson).unwrap();
	let object = json.as_object().unwrap();
	object.get("features").unwrap().as_array().unwrap().0.clone()
}

fn property(feature: &versatiles_core::json::JsonValue, key: &str) -> String {
	let properties = feature.as_object().unwrap().get("properties").unwrap();
	properties
		.as_object()
		.unwrap()
		.get(key)
		.unwrap()
		.as_str()
		.unwrap()
		.to_string()
}

#[test]
fn e2e_export_tile_as_geojson() {
	let input = get_testdata("berlin.mbtiles");
	let o = versatiles_output(&format!(
		"export tile -q {input} 14/8801/5373 --as geojson --layer buildings"
	));
	assert!(o.success, "{}", o.stderr);
	let features = features(&o.stdout);
	assert_ne!(features.len(), 0, "the tile has buildings");
	for feature in &features {
		assert_eq!(property(feature, "$layer"), "buildings");
		assert_eq!(property(feature, "$tile"), "14/8801/5373");
	}

	let o = versatiles_output(&format!(
		"export tile -q {input} 14/8801/5373 --layer buildings -o tile.pbf"
	));
	assert!(!o.success);
	assert_contains!(&o.stderr, "--layer selects the layers of GeoJSON output");
}

#[test]
fn e2e_export_area_as_geojson() {
	let input = get_testdata("berlin.mbtiles");
	let (_dir, output) = get_temp_output("buildings.geojson");

	versatiles_run(&format!(
		"export area -q {input} --bbox 13.39,52.515,13.41,52.525 --zoom 14 --layer buildings --clip -o {}",
		output.to_str().unwrap()
	));

	let features = features(&std::fs::read_to_string(&output).unwrap());
	assert!(features.len() > 100, "{} features", features.len());
	let tiles: std::collections::BTreeSet<String> = features.iter().map(|f| property(f, "$tile")).collect();
	assert!(tiles.len() > 1, "the bbox crosses tile edges: {tiles:?}");
	assert!(features.iter().all(|f| property(f, "$layer") == "buildings"));
}
