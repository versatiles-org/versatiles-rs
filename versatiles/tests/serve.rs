mod test_utilities;

use std::{net::TcpListener, process::Child, thread, time::Duration};
use test_utilities::*;
use versatiles_core::json::JsonValue;

#[tokio::test]
async fn serve_local_file() {
	let input = get_testdata("berlin.pmtiles");
	let server = Server::new(&[&input]).await;
	assert_eq!(server.get_index().await, ["berlin"]);
	assert_eq!(
		server.get_tilejson("berlin").await,
		vec!["length: 19", "desc: Tile config for simple vector tiles schema"]
	);
}

#[tokio::test]
async fn serve_remote_url() {
	let server = Server::new(&["https://download.versatiles.org/osm.versatiles"]).await;
	assert_eq!(server.get_index().await, ["osm"]);
	assert_eq!(
		server.get_tilejson("osm").await,
		vec!["length: 26", "desc: Vector tiles based on OSM in Shortbread scheme"]
	);
}

struct Server {
	host: String,
	child: Child,
}

impl Server {
	async fn new(input: &[&str]) -> Self {
		let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
		println!("Starting server on port {}", port);
		let mut cmd = versatiles_cmd();
		cmd.args([&["serve", "-p", &port.to_string()], input].concat());
		let mut child = cmd.spawn().unwrap();

		loop {
			thread::sleep(Duration::from_millis(100));
			if child.try_wait().unwrap().is_some() {
				panic!("server process exited prematurely");
			}
			if reqwest::get(&format!("http://127.0.0.1:{port}/index.json"))
				.await
				.is_ok()
			{
				break;
			}
		}

		Self {
			host: format!("http://127.0.0.1:{port}"),
			child,
		}
	}

	fn shutdown(&mut self) {
		let _ = self.child.kill();
		let _ = self.child.wait();
	}

	async fn get_json(&self, path: &str) -> JsonValue {
		let url = format!("{}{}", self.host, path);
		let resp = reqwest::get(&url).await.unwrap();
		assert_eq!(resp.status(), 200);
		let text = resp.text().await.unwrap();

		JsonValue::parse_str(&text).unwrap()
	}

	async fn get_tilejson(&self, name: &str) -> Vec<String> {
		let json = self
			.get_json(&format!("/tiles/{name}/tiles.json"))
			.await
			.into_object()
			.unwrap();
		let desc = json.get_string("description").unwrap().unwrap_or_default();
		let length = json.get_array("vector_layers").unwrap().unwrap().len();
		vec![format!("length: {length}"), format!("desc: {desc}")]
	}

	async fn get_index(&self) -> Vec<String> {
		self
			.get_json("/tiles/index.json")
			.await
			.into_array()
			.unwrap()
			.as_string_vec()
			.unwrap()
	}
}

impl Drop for Server {
	fn drop(&mut self) {
		self.shutdown()
	}
}
