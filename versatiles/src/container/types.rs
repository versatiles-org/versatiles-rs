//! Types used for handling tile streams and probing depths in tile containers.
//!
//! This module provides types and utilities for managing tile data streams and probing the depth of tile containers.

use crate::types::{Blob, TileCoord3};
use futures::Stream;
use std::pin::Pin;

/// A type alias for a stream of tiles, where each item is a tuple containing a tile coordinate and its associated data.
pub type TilesStream<'a> = Pin<Box<dyn Stream<Item = (TileCoord3, Blob)> + Send + 'a>>;

/// Enum representing the depth of probing for a tile container.
pub enum ProbeDepth {
	/// Shallow probing depth.
	Shallow = 0,
	/// Container level probing depth.
	Container = 1,
	/// Tiles level probing depth.
	Tiles = 2,
	/// Tile contents level probing depth.
	TileContents = 3,
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::types::TileCoord3;
	use futures::{stream, StreamExt};

	#[tokio::test]
	async fn test_tiles_stream() {
		let tile_data = vec![
			(TileCoord3::new(0, 0, 0).unwrap(), Blob::from("tile1")),
			(TileCoord3::new(1, 1, 1).unwrap(), Blob::from("tile2")),
		];

		let tiles_stream: TilesStream = Box::pin(stream::iter(tile_data));

		process_tiles_stream(tiles_stream).await;
	}

	async fn process_tiles_stream(mut tiles_stream: TilesStream<'_>) {
		let mut count = 0;
		while let Some((coord, blob)) = tiles_stream.next().await {
			println!(
				"Processing tile at coord: {:?}, with data: {:?}",
				coord, blob
			);
			count += 1;
		}
		assert_eq!(count, 2);
	}

	#[test]
	fn test_probe_depth() {
		probe_container(ProbeDepth::Shallow);
		probe_container(ProbeDepth::Container);
		probe_container(ProbeDepth::Tiles);
		probe_container(ProbeDepth::TileContents);
	}

	fn probe_container(depth: ProbeDepth) {
		match depth {
			ProbeDepth::Shallow => println!("Performing a shallow probe"),
			ProbeDepth::Container => println!("Probing container level"),
			ProbeDepth::Tiles => println!("Probing tiles level"),
			ProbeDepth::TileContents => println!("Probing tile contents level"),
		}
	}
}
