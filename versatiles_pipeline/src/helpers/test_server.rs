//! A local HTTP server for testing arguments that take a URL.

/// Serves each `(path, content)` at its path, a 404 everywhere else, and
/// returns the base URL, e.g. `http://127.0.0.1:54321`.
///
/// The server runs until the test's runtime shuts down.
pub async fn serve_files(files: Vec<(&'static str, Vec<u8>)>) -> String {
	let mut app = axum::Router::new();
	for (path, content) in files {
		app = app.route(path, axum::routing::get(move || async move { content }));
	}
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
	let addr = listener.local_addr().unwrap();
	tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
	format!("http://{addr}")
}
