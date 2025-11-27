mod test_utilities;
use crate::test_utilities::{get_metadata, get_temp_output, get_testdata};
use assert_cmd::{Command, cargo};
use predicates::str;
use pretty_assertions::assert_eq;

#[test]
fn convert_requires_input_and_output() {
	let mut cmd = Command::new(cargo::cargo_bin!());
	cmd.arg("convert")
		.assert()
		.failure()
		.code(2)
		.stdout(str::is_empty())
		.stderr(str::contains("Usage: versatiles convert"));
}

#[test]
fn convert_mbtiles_to_versatiles() {
	let input = get_testdata("berlin.mbtiles");
	let (temp_dir, output) = get_temp_output("berlin.versatiles");

	Command::new(cargo::cargo_bin!())
		.args(["convert", input.to_str().unwrap(), output.to_str().unwrap()])
		.assert()
		.success()
		.stdout(str::is_empty());

	assert!(output.exists(), "output file was not created: {:?}", output);

	drop(temp_dir); // clean up
}

#[test]
fn convert_pmtiles_to_mbtiles_with_bbox_and_border() {
	let input = get_testdata("berlin.pmtiles");
	let (temp_dir, output) = get_temp_output("berlin-bbox.mbtiles");

	Command::new(cargo::cargo_bin!())
		.args([
			"convert",
			"--bbox",
			"13.0,52.0,13.8,52.8",
			"--bbox-border",
			"1",
			input.to_str().unwrap(),
			output.to_str().unwrap(),
		])
		.assert()
		.success()
		.stdout(str::is_empty());

	assert!(output.exists(), "output file was not created: {:?}", output);
	assert_eq!(
		get_metadata(&output),
		"{author:OpenStreetMap contributors, Geofabrik GmbH,bounds:[13.07373,52.321911,13.776855,52.683043],description:Tile config for simple vector tiles schema,license:Open Database License 1.0,maxzoom:14,minzoom:0,name:Tilemaker to Geofabrik Vector Tiles schema,tilejson:3.0.0,type:baselayer,vector_layers:[{fields:{name:String,number:String},id:addresses,maxzoom:14,minzoom:14},{fields:{kind:String},id:aerialways,maxzoom:14,minzoom:12},{fields:{admin_level:Number,maritime:Boolean},id:boundaries,maxzoom:14,minzoom:0},{fields:{admin_level:String,name:String,name_de:String,name_en:String,way_area:Number},id:boundary_labels,maxzoom:14,minzoom:2},{fields:{dummy:Number},id:buildings,maxzoom:14,minzoom:14},{fields:{kind:String},id:land,maxzoom:14,minzoom:7},{fields:{},id:ocean,maxzoom:14,minzoom:8},{fields:{kind:String,name:String,name_de:String,name_en:String,population:Number},id:place_labels,maxzoom:14,minzoom:3},{fields:{kind:String,name:String,name_de:String,name_en:String},id:public_transport,maxzoom:14,minzoom:11},{fields:{kind:String},id:sites,maxzoom:14,minzoom:14},{fields:{kind:String,name:String,name_de:String,name_en:String,ref:String,ref_cols:Number,ref_rows:Number,tunnel:Boolean},id:street_labels,maxzoom:14,minzoom:10},{fields:{kind:String,name:String,name_de:String,name_en:String,ref:String},id:street_labels_points,maxzoom:14,minzoom:12},{fields:{bridge:Boolean,kind:String,rail:Boolean,service:String,surface:String,tunnel:Boolean},id:street_polygons,maxzoom:14,minzoom:14},{fields:{bicycle:String,bridge:Boolean,horse:String,kind:String,link:Boolean,rail:Boolean,service:String,surface:String,tracktype:String,tunnel:Boolean},id:streets,maxzoom:14,minzoom:14},{fields:{kind:String,name:String,name_de:String,name_en:String},id:streets_polygons_labels,maxzoom:14,minzoom:14},{fields:{kind:String},id:water_lines,maxzoom:14,minzoom:4},{fields:{kind:String,name:String,name_de:String,name_en:String},id:water_lines_labels,maxzoom:14,minzoom:4},{fields:{kind:String},id:water_polygons,maxzoom:14,minzoom:4},{fields:{kind:String,name:String,name_de:String,name_en:String},id:water_polygons_labels,maxzoom:14,minzoom:14}],version:3.0}"
	);

	drop(temp_dir); // clean up
}

#[test]
fn convert_vpl_via_stdin() {
	let testdata_pmtiles = get_testdata("berlin.pmtiles").to_string_lossy().to_string();
	let testdata_csv = get_testdata("cities.csv").to_string_lossy().to_string();
	let stdin = format!(
		r#"
			from_container filename="{testdata_pmtiles}" |
			vector_update_properties
				data_source_path="{testdata_csv}"
				layer_name="place_labels"
				id_field_tiles="name"
				id_field_data="city_name"
		"#
	)
	.into_bytes();

	println!("STDIN:\n{}", String::from_utf8_lossy(&stdin));
	println!("STDIN:\n{stdin:?}");

	let (temp_dir, output) = get_temp_output("vpl.pmtiles");
	Command::new(cargo::cargo_bin!())
		.args(["convert", "vpl:-", output.to_str().unwrap()])
		.write_stdin(stdin)
		.assert()
		.success()
		.stdout(str::is_empty());

	assert!(output.exists(), "output file was not created: {:?}", output);
	assert_eq!(
		get_metadata(&output),
		"{author:OpenStreetMap contributors, Geofabrik GmbH,bounds:[13.07373,52.321911,13.776855,52.683043],description:Tile config for simple vector tiles schema,format:pbf,license:Open Database License 1.0,maxzoom:14,minzoom:0,name:Tilemaker to Geofabrik Vector Tiles schema,tile_format:vnd.mapbox-vector-tile,tile_schema:other,tile_type:vector,tilejson:3.0.0,type:baselayer,vector_layers:[{fields:{name:String,number:String},id:addresses,maxzoom:14,minzoom:14},{fields:{kind:String},id:aerialways,maxzoom:14,minzoom:12},{fields:{admin_level:Number,maritime:Boolean},id:boundaries,maxzoom:14,minzoom:0},{fields:{admin_level:String,name:String,name_de:String,name_en:String,way_area:Number},id:boundary_labels,maxzoom:14,minzoom:2},{fields:{dummy:Number},id:buildings,maxzoom:14,minzoom:14},{fields:{kind:String},id:land,maxzoom:14,minzoom:7},{fields:{},id:ocean,maxzoom:14,minzoom:8},{fields:{city_id:automatically added field,city_population:automatically added field,kind:String,name:String,name_de:String,name_en:String,population:Number},id:place_labels,maxzoom:14,minzoom:3},{fields:{kind:String,name:String,name_de:String,name_en:String},id:public_transport,maxzoom:14,minzoom:11},{fields:{kind:String},id:sites,maxzoom:14,minzoom:14},{fields:{kind:String,name:String,name_de:String,name_en:String,ref:String,ref_cols:Number,ref_rows:Number,tunnel:Boolean},id:street_labels,maxzoom:14,minzoom:10},{fields:{kind:String,name:String,name_de:String,name_en:String,ref:String},id:street_labels_points,maxzoom:14,minzoom:12},{fields:{bridge:Boolean,kind:String,rail:Boolean,service:String,surface:String,tunnel:Boolean},id:street_polygons,maxzoom:14,minzoom:14},{fields:{bicycle:String,bridge:Boolean,horse:String,kind:String,link:Boolean,rail:Boolean,service:String,surface:String,tracktype:String,tunnel:Boolean},id:streets,maxzoom:14,minzoom:14},{fields:{kind:String,name:String,name_de:String,name_en:String},id:streets_polygons_labels,maxzoom:14,minzoom:14},{fields:{kind:String},id:water_lines,maxzoom:14,minzoom:4},{fields:{kind:String,name:String,name_de:String,name_en:String},id:water_lines_labels,maxzoom:14,minzoom:4},{fields:{kind:String},id:water_polygons,maxzoom:14,minzoom:4},{fields:{kind:String,name:String,name_de:String,name_en:String},id:water_polygons_labels,maxzoom:14,minzoom:14}],version:3.0}"
	);

	drop(temp_dir); // clean up
}
