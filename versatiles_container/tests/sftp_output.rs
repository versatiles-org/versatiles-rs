//! End-to-end tests for the registry's SFTP output wiring.
//!
//! `publish_remote` is covered in `versatiles_core`, where the in-process server
//! lives. What it cannot reach is the layer above: that a conversion actually
//! uploads to a staging name, that a clean run publishes it under the name the
//! user asked for, and that a run which could not read every tile leaves the
//! previously published tileset alone. That wiring is what these cover.

#![cfg(feature = "sftp")]

use anyhow::Result;
use versatiles_container::*;
use versatiles_core::{TilePyramid, io::test_sftp_server::TestSftpServer};

fn runtime(fail: bool) -> TilesRuntime {
	let runtime = TilesRuntime::builder()
		.silent_progress(true)
		.abort_on_error(true)
		.build();
	if fail {
		// A tile the reader could not deliver: the writer still succeeds, so this
		// is the case where an incomplete upload could reach the destination.
		runtime.record_error("synthetic test source", &anyhow::anyhow!("simulated read failure"));
	}
	runtime
}

async fn convert_to(destination: &str, max_zoom: u8, fail: bool) -> Result<()> {
	let runtime = runtime(fail);
	let source = runtime.reader_from_str("../testdata/berlin.mbtiles").await?;
	convert_tiles_container_to_str(
		source,
		TilesConverterParameters {
			tile_pyramid: Some(TilePyramid::new_full_up_to(max_zoom)),
			..Default::default()
		},
		destination,
		runtime,
	)
	.await
}

#[tokio::test(flavor = "current_thread")]
async fn a_clean_run_publishes_under_the_requested_name() -> Result<()> {
	let server = TestSftpServer::start().await;
	let destination = server.url("/out.versatiles").to_string();

	convert_to(&destination, 3, false).await?;

	let published = server.read_file("/out.versatiles").await;
	assert!(!published.is_empty(), "nothing was published");
	assert!(
		server.read_file("/.out.versatiles.tmp").await.is_empty(),
		"the staging upload must not survive a success"
	);
	Ok(())
}

/// The upload has to go to a staging name, not straight to the destination —
/// otherwise a failure leaves a corrupt file under the name clients fetch.
#[tokio::test(flavor = "current_thread")]
async fn a_failed_run_leaves_the_published_tileset_alone() -> Result<()> {
	let server = TestSftpServer::start().await;
	let destination = server.url("/out.versatiles").to_string();

	convert_to(&destination, 3, false).await?;
	let before = server.read_file("/out.versatiles").await;

	let result = convert_to(&destination, 2, true).await;
	assert!(result.is_err(), "a recorded read error must fail the run");

	assert_eq!(
		server.read_file("/out.versatiles").await,
		before,
		"the published tileset must be byte-identical after a failed re-publish"
	);
	assert!(
		!server.read_file("/out.incomplete.versatiles").await.is_empty(),
		"the incomplete upload should have been kept under its own name"
	);
	assert!(
		server.read_file("/.out.versatiles.tmp").await.is_empty(),
		"the staging name must be free again"
	);
	Ok(())
}

/// Replacing an existing remote file takes the three-step rename, and must not
/// leave the `.old` copy behind.
#[tokio::test(flavor = "current_thread")]
async fn republishing_replaces_and_leaves_no_leftovers() -> Result<()> {
	let server = TestSftpServer::start().await;
	let destination = server.url("/out.versatiles").to_string();

	convert_to(&destination, 4, false).await?;
	let first = server.read_file("/out.versatiles").await;

	convert_to(&destination, 2, false).await?;
	let second = server.read_file("/out.versatiles").await;

	assert!(!second.is_empty());
	assert_ne!(second, first, "the tileset should have been replaced");
	assert!(
		server.read_file("/out.versatiles.old").await.is_empty(),
		"the previous upload must not be left lying around"
	);
	assert!(
		server.read_file("/.out.versatiles.tmp").await.is_empty(),
		"the staging name must be free again"
	);
	Ok(())
}
