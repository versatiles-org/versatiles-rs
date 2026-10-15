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

// --- with a request in flight ---

/// How long the slow source takes for each of its answers. Long enough that a
/// tile request is still running a good while after it was sent; short enough
/// that the server, which needs a few answers to open the source, is up soon.
const SOURCE_DELAY_MS: u64 = 400;

/// A server whose only source answers slowly, and a tile request that is on
/// its way through it.
async fn server_with_a_request_in_flight() -> (Child, tokio::task::JoinHandle<reqwest::Result<reqwest::StatusCode>>) {
	let server = TestHttpServer::shared();
	let url = server.slow_url("berlin.pmtiles", SOURCE_DELAY_MS);
	let (host, child) = spawn_server(&[&format!("[berlin]{url}")], "/tiles/index.json").await;

	// Opening the source has already asked it for a few things.
	let before = server.requests_to(&url);
	let request = tokio::spawn(async move {
		let response = reqwest::get(format!("{host}/tiles/berlin/12/2200/1343")).await?;
		let status = response.status();
		response.bytes().await?;
		Ok(status)
	});

	// Wait until the source is asked, which is when the server is answering the
	// request. A fixed sleep was not enough on a slow runner: the signal came
	// before the request did, the server had nothing to wait for and exited
	// cleanly, and the test saw exit code 0 instead of the forced exit.
	let start = Instant::now();
	while server.requests_to(&url) == before {
		assert!(
			start.elapsed() < Duration::from_secs(10),
			"the request never reached the source"
		);
		tokio::time::sleep(Duration::from_millis(5)).await;
	}
	assert!(!request.is_finished(), "the request should still be running");

	(child, request)
}

/// The point of the graceful shutdown (#282): a request that is being
/// answered when the signal arrives is answered, and then the server exits.
#[tokio::test]
async fn e2e_serve_finishes_a_request_that_is_in_flight() {
	let (mut child, request) = server_with_a_request_in_flight().await;

	send_signal(&child, "-TERM");
	let status = request
		.await
		.unwrap()
		.expect("the request should be answered, not cut off");
	assert_eq!(status, 200);

	let exit = wait_for_exit(&mut child, Duration::from_secs(3)).await;
	assert!(exit.success(), "expected a clean exit, got {exit}");
}

/// Ctrl+C twice gets out at once: the second signal does not wait for the
/// request, and the exit code says the shutdown was not a clean one.
#[tokio::test]
async fn e2e_serve_exits_at_once_on_a_second_signal() {
	let (mut child, request) = server_with_a_request_in_flight().await;

	send_signal(&child, "-INT");
	tokio::time::sleep(Duration::from_millis(50)).await;
	send_signal(&child, "-INT");

	let exit = wait_for_exit(&mut child, Duration::from_millis(SOURCE_DELAY_MS / 2)).await;
	assert_eq!(exit.code(), Some(1), "expected the forced exit, got {exit}");
	assert!(request.await.unwrap().is_err(), "the request was cut off");
}
