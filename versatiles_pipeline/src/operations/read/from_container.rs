//! # From‑container read operation
//!
//! This module defines an [`Operation`] that streams tiles out of a **single
//! tile container** (e.g. `*.versatiles`, MBTiles, PMTiles, TAR bundles).
//! It adapts the container’s [`TileSource`] interface to
//! [`TileSource`] so that the rest of the pipeline can treat it like any
//! other data source.

use std::fmt::Debug;

use anyhow::Result;
use async_trait::async_trait;
use versatiles_container::{DataLocation, Tile, TileSource, TileSourceMetadata};
use versatiles_core::{TileBBox, TileJSON, TilePyramid, TileStream};
use versatiles_derive::context;

use crate::{PipelineFactory, vpl::VPLNode};

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Reads a tile container, such as a `*.versatiles`, `*.mbtiles`, `*.pmtiles` or `*.tar` file.
///
/// `filename` takes a local path or a URL — `filename="world.versatiles"` as
/// readily as `filename="https://example.com/world.versatiles"`. Run
/// `versatiles help source` for the URL and authentication syntax.
///
/// `ssh_identity` names a key for this source alone, overriding `--ssh-identity`
/// and `VERSATILES_SSH_IDENTITY`, so one pipeline can read from two SFTP hosts
/// that need different keys. It is ignored for every other scheme, and naming a
/// key ties the `.vpl` file to machines that have it.
struct Args {
	/// Path to the container, or an `http`, `https` or `sftp` URL.
	filename: String,

	/// Private key for this one `sftp://` source. Defaults to the global setting.
	ssh_identity: Option<String>,
}

#[derive(Debug)]
/// Concrete [`TileSource`] that merely forwards every request to an
/// underlying container [`TileSource`].  A cached copy of the
/// container’s [`TileJSON`] metadata is kept so downstream stages can query
/// bounds and zoom levels without touching the reader again.
struct Operation {
	source: Box<dyn TileSource>,
	metadata: TileSourceMetadata,
	tilejson: TileJSON,
}

impl Operation {
	#[context("Failed to build from_container operation in VPL node {:?}", vpl_node.name)]
	async fn build(vpl_node: VPLNode, factory: &PipelineFactory) -> Result<Box<dyn TileSource>> {
		let args = Args::from_vpl_node(&vpl_node)?;
		let source = factory
			.reader_with_ssh_identity(DataLocation::try_from(&args.filename)?, args.ssh_identity.as_deref())
			.await?;
		let tile_pyramid = source.tile_pyramid().await?;
		let mut metadata = source.metadata().clone();
		metadata.set_tile_pyramid(tile_pyramid.as_ref().clone());
		let mut tilejson = source.tilejson().clone();
		metadata.update_tilejson(&mut tilejson);

		Ok(Box::new(Self {
			source,
			metadata,
			tilejson,
		}) as Box<dyn TileSource>)
	}
}

#[async_trait]
impl TileSource for Operation {
	fn source_type(&self) -> std::sync::Arc<versatiles_container::SourceType> {
		self.source.source_type()
	}

	/// Return the reader's technical parameters (compression, tile size,
	/// etc.) without performing any I/O.
	fn metadata(&self) -> &TileSourceMetadata {
		&self.metadata
	}

	/// Expose the container's `TileJSON` so that consumers can inspect
	/// bounds, zoom range and other dataset metadata.
	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	async fn tile_pyramid(&self) -> Result<std::sync::Arc<TilePyramid>> {
		self.source.tile_pyramid().await
	}

	/// Stream raw tile blobs intersecting the bounding box by delegating to
	/// `TileSource::tile_stream`.
	#[context("Failed to get tile stream for bbox: {:?}", bbox)]
	async fn tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		log::trace!("from_container::tile_stream {bbox:?}");
		self.source.tile_stream(bbox).await
	}

	async fn tile_coord_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, ()>> {
		self.source.tile_coord_stream(bbox).await
	}
}

crate::operations::macros::define_read_factory!("from_container", Args, Operation);

#[cfg(test)]
pub fn operation_from_reader(reader: Box<dyn TileSource>) -> Box<dyn TileSource> {
	let metadata = reader.metadata().clone();
	let mut tilejson = reader.tilejson().clone();
	metadata.update_tilejson(&mut tilejson);
	Box::new(Operation {
		source: reader,
		metadata,
		tilejson,
	}) as Box<dyn TileSource>
}

#[cfg(test)]
mod tests {
	use std::{
		path::PathBuf,
		sync::{Arc, Mutex},
	};

	use futures::future::BoxFuture;
	use versatiles_container::TilesRuntime;
	use versatiles_core::TileCompression::Uncompressed;

	use super::*;
	use crate::helpers::dummy_vector_source::DummyVectorSource;

	/// A factory whose reader records the identity it was handed.
	///
	/// `dir` is the base a relative identity resolves against, the same base
	/// `filename` uses.
	fn recording_factory(dir: &str) -> (PipelineFactory, Arc<Mutex<Vec<Option<PathBuf>>>>) {
		let seen: Arc<Mutex<Vec<Option<PathBuf>>>> = Arc::new(Mutex::new(Vec::new()));
		let recorder = Arc::clone(&seen);
		let callback = Box::new(
			move |_location: DataLocation, ssh_identity: Option<PathBuf>| -> BoxFuture<Result<Box<dyn TileSource>>> {
				recorder.lock().unwrap().push(ssh_identity);
				Box::pin(async move {
					Ok(Box::new(DummyVectorSource::new(&[("dummy", &[&[("a", "b")]])], None)) as Box<dyn TileSource>)
				})
			},
		);
		let factory = PipelineFactory::new_default(
			DataLocation::from(PathBuf::from(dir)),
			callback,
			TilesRuntime::new_silent(),
		);
		(factory, seen)
	}

	#[tokio::test]
	async fn ssh_identity_reaches_the_reader() -> Result<()> {
		let (factory, seen) = recording_factory("/base");
		factory
			.operation_from_vpl("from_container filename=\"sftp://host/world.versatiles\" ssh_identity=\"/keys/a\"")
			.await?;

		assert_eq!(*seen.lock().unwrap(), vec![Some(PathBuf::from("/keys/a"))]);
		Ok(())
	}

	#[tokio::test]
	async fn a_relative_ssh_identity_resolves_against_the_vpl_file() -> Result<()> {
		// `filename` is relative to the VPL file, so an identity beside it has to
		// resolve the same way — otherwise the two paths in one node would mean
		// different things.
		let (factory, seen) = recording_factory("/base");
		factory
			.operation_from_vpl("from_container filename=\"sftp://host/world.versatiles\" ssh_identity=\"keys/a\"")
			.await?;

		assert_eq!(*seen.lock().unwrap(), vec![Some(PathBuf::from("/base/keys/a"))]);
		Ok(())
	}

	#[tokio::test]
	async fn without_ssh_identity_the_reader_is_told_nothing() -> Result<()> {
		// The runtime's own identity, if any, then applies — deciding that is the
		// registry's job, not this operation's.
		let (factory, seen) = recording_factory("/base");
		factory
			.operation_from_vpl("from_container filename=\"sftp://host/world.versatiles\"")
			.await?;

		assert_eq!(*seen.lock().unwrap(), vec![None]);
		Ok(())
	}

	#[tokio::test]
	async fn test_vector() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let operation = factory
			.operation_from_vpl("from_container filename=\"test.pbf\"")
			.await?;

		assert_eq!(
			operation
				.tilejson()
				.to_pretty_lines(10)
				.iter()
				.map(std::string::String::as_str)
				.collect::<Vec<_>>(),
			[
				"{",
				"  \"bounds\": [",
				"    -180,",
				"    -85.051129,",
				"    180,",
				"    85.051129",
				"  ],",
				"  \"maxzoom\": 8,",
				"  \"minzoom\": 0,",
				"  \"name\": \"dummy vector source\",",
				"  \"tile_format\": \"application/vnd.mapbox-vector-tile\",",
				"  \"tile_schema\": \"other\",",
				"  \"tile_type\": \"vector\",",
				"  \"tilejson\": \"3.0.0\"",
				"}"
			]
		);

		let mut stream = operation
			.tile_stream(TileBBox::from_min_and_max(3, 1, 1, 2, 3)?)
			.await?;

		let mut n = 0;
		while let Some((coord, tile)) = stream.next().await {
			assert!(tile.into_blob(&Uncompressed)?.len() > 50);
			assert!(coord.x >= 1 && coord.x <= 2);
			assert!(coord.y >= 1 && coord.y <= 3);
			assert_eq!(coord.level, 3);
			n += 1;
		}
		assert_eq!(n, 6);

		Ok(())
	}

	#[tokio::test]
	async fn test_raster() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let operation = factory
			.operation_from_vpl("from_container filename=\"abc.png\"")
			.await?;

		assert_eq!(
			operation
				.tilejson()
				.to_pretty_lines(10)
				.iter()
				.map(std::string::String::as_str)
				.collect::<Vec<_>>(),
			[
				"{",
				"  \"bounds\": [",
				"    -180,",
				"    -85.051129,",
				"    180,",
				"    85.051129",
				"  ],",
				"  \"maxzoom\": 8,",
				"  \"minzoom\": 0,",
				"  \"name\": \"dummy raster source\",",
				"  \"tile_format\": \"image/png\",",
				"  \"tile_schema\": \"rgb\",",
				"  \"tile_type\": \"raster\",",
				"  \"tilejson\": \"3.0.0\"",
				"}"
			]
		);

		let mut stream = operation
			.tile_stream(TileBBox::from_min_and_max(3, 1, 1, 2, 3)?)
			.await?;

		let mut n = 0;
		while let Some((coord, tile)) = stream.next().await {
			assert!(tile.into_blob(&Uncompressed)?.len() > 50);
			assert!(coord.x >= 1 && coord.x <= 2);
			assert!(coord.y >= 1 && coord.y <= 3);
			assert_eq!(coord.level, 3);
			n += 1;
		}
		assert_eq!(n, 6);

		Ok(())
	}
}
