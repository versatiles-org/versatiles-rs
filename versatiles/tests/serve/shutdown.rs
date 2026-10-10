//! `versatiles serve` stops gracefully on SIGTERM and SIGINT (#282).

#![cfg(unix)]

use std::{
	process::{Child, Command, ExitStatus},
	time::{Duration, Instant},
};

use crate::test_utilities::*;

fn send_signal(child: &Child, signal: &str) {
	let status = Command::new("kill")
		.args([signal, &child.id().to_string()])
		.status()
		.expect("failed to run kill");
	assert!(status.success(), "kill {signal} failed");
}

/// Waits for the server to exit by itself, and kills it if it does not.
async fn wait_for_exit(child: &mut Child, limit: Duration) -> ExitStatus {
	let start = Instant::now();
	loop {
		if let Some(status) = child.try_wait().expect("failed to poll the server") {
			return status;
		}
		if start.elapsed() > limit {
			let _ = child.kill();
			let _ = child.wait();
			panic!("server did not exit within {limit:?} of the signal");
		}
		tokio::time::sleep(Duration::from_millis(20)).await;
	}
}

/// A process ended by a signal has no exit code, so `success()` tells a
/// shutdown the server carried out from one the default action did for it.
#[rstest::rstest]
#[case("-TERM")]
#[case("-INT")]
#[tokio::test]
async fn e2e_serve_shuts_down_gracefully_on(#[case] signal: &str) {
	let input = get_testdata("berlin.pmtiles");
	let (host, mut child) = spawn_server(&[&input], "/tiles/index.json").await;

	// A client that keeps its connection open afterwards, as a proxy does. An
	// idle connection must not hold the shutdown up until the drain limit.
	let client = reqwest::Client::new();
	let response = client
		.get(format!("{host}/tiles/berlin/tiles.json"))
		.send()
		.await
		.unwrap();
	assert_eq!(response.status(), 200);
	response.bytes().await.unwrap();

	send_signal(&child, signal);
	let status = wait_for_exit(&mut child, Duration::from_secs(3)).await;
	assert!(status.success(), "expected a clean exit after {signal}, got {status}");

	// The port is closed, so a proxy in front sees a refused connection.
	assert!(client.get(format!("{host}/tiles/index.json")).send().await.is_err());
}
