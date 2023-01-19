use crate::opencloudtiles::{
	lib::{ProgressBar, TileBBox, TileCoord2},
	tools::get_reader,
};
use clap::Args;
use itertools::Itertools;
use rayon::prelude::{ParallelBridge, ParallelIterator};
use std::sync::Mutex;

#[derive(Args)]
#[command(arg_required_else_help = true, disable_version_flag = true)]
pub struct Subcommand {
	/// tile container you want to probe
	/// supported container formats are: *.cloudtiles, *.tar, *.mbtiles
	#[arg(required = true, verbatim_doc_comment)]
	file: String,

	/// scan every tile
	#[arg(long)]
	scan: bool,
}

pub fn run(arguments: &Subcommand) {
	println!("probe {:?}", arguments.file);

	let reader = get_reader(&arguments.file);
	println!("{:#?}", reader);

	if arguments.scan {
		let pyramide = reader.get_parameters().get_level_bbox();

		let mut progress = ProgressBar::new("scan", pyramide.count_tiles());
		let mut size: usize = 0;
		let mutex_size = Mutex::new(size);

		for (level, bbox_tiles) in pyramide.iter_levels() {
			let bbox_blocks = bbox_tiles.clone().scale_down(256);
			for TileCoord2 { x, y } in bbox_blocks.iter_coords() {
				let mut bbox_block = bbox_tiles.clone();
				bbox_block.intersect_bbox(&TileBBox::new(
					x * 256,
					y * 256,
					x * 256 + 255,
					y * 256 + 255,
				));

				bbox_block
					.iter_bbox_row_slices(1024)
					.par_bridge()
					.for_each(|row_bbox: TileBBox| {
						let mut size_sum = 0;
						let width = 2u64.pow(level as u32);
						reader
							.get_bbox_tile_vec(level, &row_bbox)
							.iter()
							.sorted_by_cached_key(|(coord, _blob)| coord.y * width + coord.x)
							.for_each(|(_coord, blob)| size_sum += blob.len());

						*mutex_size.lock().unwrap() += size_sum;
					});

				progress.inc(bbox_block.count_tiles());
			}
		}

		progress.finish();

		size = *mutex_size.lock().unwrap();
		println!("size {}", size);
	}
}
