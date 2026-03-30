use anyhow::Result;
use versatiles_container::TilesRuntime;
use versatiles_core::TileBBox;

#[derive(clap::Args, Debug)]
#[command(arg_required_else_help = true, disable_version_flag = true)]
/// Count the number of tiles in a tile source.
pub struct CountTiles {
	/// Tile container to read (path, URL, or data source expression).
	/// Run `versatiles help source` for syntax details.
	#[arg(value_name = "INPUT_FILE", verbatim_doc_comment)]
	input: String,

	/// Only count tiles at this zoom level. If not specified, all levels are counted.
	#[arg(long)]
	level: Option<u8>,
}

pub async fn run(args: &CountTiles, runtime: &TilesRuntime) -> Result<()> {
	let reader = runtime.get_reader_from_str(&args.input).await?;
	let pyramid = &reader.metadata().bbox_pyramid;

	let levels: Vec<u8> = if let Some(level) = args.level {
		vec![level]
	} else {
		let min = pyramid.get_level_min().unwrap_or(0);
		let max = pyramid.get_level_max().unwrap_or(0);
		(min..=max).collect()
	};

	let mut total = 0u64;
	for level in &levels {
		let bbox = pyramid.intersected_bbox(&TileBBox::new_full(*level)?)?;
		let count = if bbox.is_empty() {
			0
		} else {
			reader.get_tile_coord_stream(bbox).await?.drain_and_count().await
		};
		println!("level {level:2}: {count}");
		total += count;
	}

	if levels.len() > 1 {
		println!("total:    {total}");
	}

	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;
	use versatiles::runtime::create_test_runtime;

	#[tokio::test]
	async fn test_count_all_levels() {
		let runtime = create_test_runtime();
		// Just verify it doesn't error — output goes to stdout
		run(
			&CountTiles {
				input: "../testdata/berlin.mbtiles".into(),
				level: None,
			},
			&runtime,
		)
		.await
		.unwrap();
	}

	#[tokio::test]
	async fn test_count_single_level() {
		let runtime = create_test_runtime();
		run(
			&CountTiles {
				input: "../testdata/berlin.mbtiles".into(),
				level: Some(0),
			},
			&runtime,
		)
		.await
		.unwrap();
	}
}
