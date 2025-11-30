mod test_utilities;

use std::{net::TcpListener, process::Child, thread, time::Duration};
use test_utilities::*;
use versatiles_core::json::JsonValue;

#[tokio::test]
async fn serve_local_file() {
	let input = get_testdata("berlin.pmtiles");
	let server = Server::new(&[&input]).await;
	assert_eq!(server.get_index().await, ["berlin"]);
	assert_eq!(server.get_tilejson_layer_count("berlin").await, 19);
}

struct Server {
	host: String,
	child: Child,
}

impl Server {
	async fn new(input: &[&str]) -> Self {
		let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
		let mut cmd = versatiles_cmd();
		cmd.args([&["serve", "-p", &port.to_string()], input].concat());
		let child = cmd.spawn().unwrap();
		thread::sleep(Duration::from_millis(100));
		Self {
			host: format!("http://127.0.0.1:{port}"),
			child,
		}
	}

	async fn get_json(&self, path: &str) -> JsonValue {
		let url = format!("{}{}", self.host, path);
		let resp = reqwest::get(&url).await.unwrap();
		assert_eq!(resp.status(), 200);
		let text = resp.text().await.unwrap();

		JsonValue::parse_str(&text).unwrap()
	}

	async fn get_tilejson_layer_count(&self, name: &str) -> usize {
		self
			.get_json(&format!("/tiles/{name}/tiles.json"))
			.await
			.into_object()
			.unwrap()
			.get_array("vector_layers")
			.unwrap()
			.unwrap()
			.len()
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
		let _ = self.child.kill();
		let _ = self.child.wait();
	}
}
