use anyhow::Result;
use std::io::Write;
use versatiles_container::TilesRuntime;

#[derive(clap::Args, Debug)]
#[command(arg_required_else_help = true, disable_version_flag = true)]
/// Print the TileJSON metadata of a container to stdout.
pub struct PrintTilejson {
	/// Input file
	#[arg(value_name = "INPUT_FILE")]
	input: String,

	/// Pretty-print the output
	#[arg(long, default_value_t = false, short = 'p')]
	pretty: bool,
}

pub async fn run(args: &PrintTilejson, runtime: TilesRuntime) -> Result<()> {
	let tilejson = fetch_tilejson(args, runtime).await?;
	std::io::stdout().write_all(tilejson.as_bytes())?;
	Ok(())
}

async fn fetch_tilejson(args: &PrintTilejson, runtime: TilesRuntime) -> Result<String> {
	let reader = runtime.get_reader_from_str(&args.input).await?;

	Ok(if args.pretty {
		reader.tilejson().as_pretty_lines(80).join("\n")
	} else {
		reader.tilejson().as_string()
	})
}

#[cfg(test)]
mod tests {
	use super::*;
	use pretty_assertions::assert_eq;
	use versatiles::runtime::create_test_runtime;

	#[tokio::test]
	async fn test_print_tilejson() {
		let output = fetch_tilejson(
			&PrintTilejson {
				input: "../testdata/berlin.mbtiles".into(),
				pretty: false,
			},
			create_test_runtime(),
		)
		.await
		.unwrap();
		assert_eq!(
			output,
			"{\"author\":\"OpenStreetMap contributors, Geofabrik GmbH\",\"bounds\":[13.08283,52.33446,13.762245,52.6783],\"description\":\"Tile config for simple vector tiles schema\",\"license\":\"Open Database License 1.0\",\"maxzoom\":14,\"minzoom\":0,\"name\":\"Tilemaker to Geofabrik Vector Tiles schema\",\"tilejson\":\"3.0.0\",\"type\":\"baselayer\",\"vector_layers\":[{\"fields\":{\"name\":\"String\",\"number\":\"String\"},\"id\":\"addresses\",\"maxzoom\":14,\"minzoom\":14},{\"fields\":{\"kind\":\"String\"},\"id\":\"aerialways\",\"maxzoom\":14,\"minzoom\":12},{\"fields\":{\"admin_level\":\"Number\",\"maritime\":\"Boolean\"},\"id\":\"boundaries\",\"maxzoom\":14,\"minzoom\":0},{\"fields\":{\"admin_level\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\",\"way_area\":\"Number\"},\"id\":\"boundary_labels\",\"maxzoom\":14,\"minzoom\":2},{\"fields\":{\"dummy\":\"Number\"},\"id\":\"buildings\",\"maxzoom\":14,\"minzoom\":14},{\"fields\":{\"kind\":\"String\"},\"id\":\"land\",\"maxzoom\":14,\"minzoom\":7},{\"fields\":{},\"id\":\"ocean\",\"maxzoom\":14,\"minzoom\":8},{\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\",\"population\":\"Number\"},\"id\":\"place_labels\",\"maxzoom\":14,\"minzoom\":3},{\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\"},\"id\":\"public_transport\",\"maxzoom\":14,\"minzoom\":11},{\"fields\":{\"kind\":\"String\"},\"id\":\"sites\",\"maxzoom\":14,\"minzoom\":14},{\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\",\"ref\":\"String\",\"ref_cols\":\"Number\",\"ref_rows\":\"Number\",\"tunnel\":\"Boolean\"},\"id\":\"street_labels\",\"maxzoom\":14,\"minzoom\":10},{\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\",\"ref\":\"String\"},\"id\":\"street_labels_points\",\"maxzoom\":14,\"minzoom\":12},{\"fields\":{\"bridge\":\"Boolean\",\"kind\":\"String\",\"rail\":\"Boolean\",\"service\":\"String\",\"surface\":\"String\",\"tunnel\":\"Boolean\"},\"id\":\"street_polygons\",\"maxzoom\":14,\"minzoom\":14},{\"fields\":{\"bicycle\":\"String\",\"bridge\":\"Boolean\",\"horse\":\"String\",\"kind\":\"String\",\"link\":\"Boolean\",\"rail\":\"Boolean\",\"service\":\"String\",\"surface\":\"String\",\"tracktype\":\"String\",\"tunnel\":\"Boolean\"},\"id\":\"streets\",\"maxzoom\":14,\"minzoom\":14},{\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\"},\"id\":\"streets_polygons_labels\",\"maxzoom\":14,\"minzoom\":14},{\"fields\":{\"kind\":\"String\"},\"id\":\"water_lines\",\"maxzoom\":14,\"minzoom\":4},{\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\"},\"id\":\"water_lines_labels\",\"maxzoom\":14,\"minzoom\":4},{\"fields\":{\"kind\":\"String\"},\"id\":\"water_polygons\",\"maxzoom\":14,\"minzoom\":4},{\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\"},\"id\":\"water_polygons_labels\",\"maxzoom\":14,\"minzoom\":14}],\"version\":\"3.0\"}"
		);
	}

	#[tokio::test]
	async fn test_pretty_print_tilejson() {
		let output = fetch_tilejson(
			&PrintTilejson {
				input: "../testdata/berlin.mbtiles".into(),
				pretty: true,
			},
			create_test_runtime(),
		)
		.await
		.unwrap();
		assert_eq!(
			output,
			"{\n  \"author\": \"OpenStreetMap contributors, Geofabrik GmbH\",\n  \"bounds\": [13.08283, 52.33446, 13.762245, 52.6783],\n  \"description\": \"Tile config for simple vector tiles schema\",\n  \"license\": \"Open Database License 1.0\",\n  \"maxzoom\": 14,\n  \"minzoom\": 0,\n  \"name\": \"Tilemaker to Geofabrik Vector Tiles schema\",\n  \"tilejson\": \"3.0.0\",\n  \"type\": \"baselayer\",\n  \"vector_layers\": [\n    {\n      \"fields\": { \"name\": \"String\", \"number\": \"String\" },\n      \"id\": \"addresses\",\n      \"maxzoom\": 14,\n      \"minzoom\": 14\n    },\n    {\n      \"fields\": { \"kind\": \"String\" },\n      \"id\": \"aerialways\",\n      \"maxzoom\": 14,\n      \"minzoom\": 12\n    },\n    {\n      \"fields\": { \"admin_level\": \"Number\", \"maritime\": \"Boolean\" },\n      \"id\": \"boundaries\",\n      \"maxzoom\": 14,\n      \"minzoom\": 0\n    },\n    {\n      \"fields\": {\n        \"admin_level\": \"String\",\n        \"name\": \"String\",\n        \"name_de\": \"String\",\n        \"name_en\": \"String\",\n        \"way_area\": \"Number\"\n      },\n      \"id\": \"boundary_labels\",\n      \"maxzoom\": 14,\n      \"minzoom\": 2\n    },\n    {\n      \"fields\": { \"dummy\": \"Number\" },\n      \"id\": \"buildings\",\n      \"maxzoom\": 14,\n      \"minzoom\": 14\n    },\n    {\n      \"fields\": { \"kind\": \"String\" },\n      \"id\": \"land\",\n      \"maxzoom\": 14,\n      \"minzoom\": 7\n    },\n    { \"fields\": {  }, \"id\": \"ocean\", \"maxzoom\": 14, \"minzoom\": 8 },\n    {\n      \"fields\": {\n        \"kind\": \"String\",\n        \"name\": \"String\",\n        \"name_de\": \"String\",\n        \"name_en\": \"String\",\n        \"population\": \"Number\"\n      },\n      \"id\": \"place_labels\",\n      \"maxzoom\": 14,\n      \"minzoom\": 3\n    },\n    {\n      \"fields\": {\n        \"kind\": \"String\",\n        \"name\": \"String\",\n        \"name_de\": \"String\",\n        \"name_en\": \"String\"\n      },\n      \"id\": \"public_transport\",\n      \"maxzoom\": 14,\n      \"minzoom\": 11\n    },\n    {\n      \"fields\": { \"kind\": \"String\" },\n      \"id\": \"sites\",\n      \"maxzoom\": 14,\n      \"minzoom\": 14\n    },\n    {\n      \"fields\": {\n        \"kind\": \"String\",\n        \"name\": \"String\",\n        \"name_de\": \"String\",\n        \"name_en\": \"String\",\n        \"ref\": \"String\",\n        \"ref_cols\": \"Number\",\n        \"ref_rows\": \"Number\",\n        \"tunnel\": \"Boolean\"\n      },\n      \"id\": \"street_labels\",\n      \"maxzoom\": 14,\n      \"minzoom\": 10\n    },\n    {\n      \"fields\": {\n        \"kind\": \"String\",\n        \"name\": \"String\",\n        \"name_de\": \"String\",\n        \"name_en\": \"String\",\n        \"ref\": \"String\"\n      },\n      \"id\": \"street_labels_points\",\n      \"maxzoom\": 14,\n      \"minzoom\": 12\n    },\n    {\n      \"fields\": {\n        \"bridge\": \"Boolean\",\n        \"kind\": \"String\",\n        \"rail\": \"Boolean\",\n        \"service\": \"String\",\n        \"surface\": \"String\",\n        \"tunnel\": \"Boolean\"\n      },\n      \"id\": \"street_polygons\",\n      \"maxzoom\": 14,\n      \"minzoom\": 14\n    },\n    {\n      \"fields\": {\n        \"bicycle\": \"String\",\n        \"bridge\": \"Boolean\",\n        \"horse\": \"String\",\n        \"kind\": \"String\",\n        \"link\": \"Boolean\",\n        \"rail\": \"Boolean\",\n        \"service\": \"String\",\n        \"surface\": \"String\",\n        \"tracktype\": \"String\",\n        \"tunnel\": \"Boolean\"\n      },\n      \"id\": \"streets\",\n      \"maxzoom\": 14,\n      \"minzoom\": 14\n    },\n    {\n      \"fields\": {\n        \"kind\": \"String\",\n        \"name\": \"String\",\n        \"name_de\": \"String\",\n        \"name_en\": \"String\"\n      },\n      \"id\": \"streets_polygons_labels\",\n      \"maxzoom\": 14,\n      \"minzoom\": 14\n    },\n    {\n      \"fields\": { \"kind\": \"String\" },\n      \"id\": \"water_lines\",\n      \"maxzoom\": 14,\n      \"minzoom\": 4\n    },\n    {\n      \"fields\": {\n        \"kind\": \"String\",\n        \"name\": \"String\",\n        \"name_de\": \"String\",\n        \"name_en\": \"String\"\n      },\n      \"id\": \"water_lines_labels\",\n      \"maxzoom\": 14,\n      \"minzoom\": 4\n    },\n    {\n      \"fields\": { \"kind\": \"String\" },\n      \"id\": \"water_polygons\",\n      \"maxzoom\": 14,\n      \"minzoom\": 4\n    },\n    {\n      \"fields\": {\n        \"kind\": \"String\",\n        \"name\": \"String\",\n        \"name_de\": \"String\",\n        \"name_en\": \"String\"\n      },\n      \"id\": \"water_polygons_labels\",\n      \"maxzoom\": 14,\n      \"minzoom\": 14\n    }\n  ],\n  \"version\": \"3.0\"\n}"
		);
	}
}
