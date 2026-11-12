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
