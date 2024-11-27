use super::{
	sources::{SourceResponse, StaticSource, TileSource},
	utils::Url,
};
use crate::{
	types::{Blob, TileCompression, TilesReaderTrait},
	utils::{optimize_compression, TargetCompression},
};
use anyhow::{bail, Result};
use axum::{
	body::Body,
	extract::State,
	http::{
		header::{ACCEPT_ENCODING, CACHE_CONTROL, CONTENT_ENCODING, CONTENT_TYPE},
		HeaderMap, Uri,
	},
	response::Response,
	routing::get,
	Router,
};
use hyper::header::{ACCESS_CONTROL_ALLOW_ORIGIN, VARY};
use std::path::Path;
use tokio::sync::oneshot::Sender;

pub struct TileServer {
	ip: String,
	port: u16,
	tile_sources: Vec<TileSource>,
	static_sources: Vec<StaticSource>,
	exit_signal: Option<Sender<()>>,
	use_best_compression: bool,
	use_api: bool,
}

impl TileServer {
	pub fn new(ip: &str, port: u16, use_best_compression: bool, use_api: bool) -> TileServer {
		TileServer {
			ip: ip.to_owned(),
			port,
			tile_sources: Vec::new(),
			static_sources: Vec::new(),
			exit_signal: None,
			use_best_compression,
			use_api,
		}
	}

	pub fn add_tile_source(&mut self, id: &str, reader: Box<dyn TilesReaderTrait>) -> Result<()> {
		log::info!("add source: id='{}', source={:?}", id, reader);

		let source = TileSource::from(reader, id)?;
		let url_prefix = &source.prefix;

		for other_tile_source in self.tile_sources.iter() {
			let other_prefix = &other_tile_source.prefix;
			if other_prefix.starts_with(url_prefix) || url_prefix.starts_with(other_prefix) {
				bail!(
					"multiple sources with the prefix '{url_prefix}' and '{other_prefix}' are defined"
				);
			};
		}

		self.tile_sources.push(source);

		Ok(())
	}

	pub fn add_static_source(&mut self, path: &Path, url_prefix: Url) -> Result<()> {
		let url_prefix = url_prefix.as_dir();

		log::info!("add static: {path:?}");
		self
			.static_sources
			.push(StaticSource::new(path, url_prefix)?);
		Ok(())
	}

	pub async fn start(&mut self) -> Result<()> {
		if self.exit_signal.is_some() {
			self.stop().await
		}

		log::info!("starting server");

		// Initialize App
		let mut router = Router::new().route("/status", get(|| async { "ready!" }));

		router = self.add_tile_sources_to_app(router);
		if self.use_api {
			router = self.add_api_to_app(router).await?;
		}
		router = self.add_static_sources_to_app(router);

		let addr = format!("{}:{}", self.ip, self.port);
		eprintln!("server starts listening on {}", addr);

		let listener = tokio::net::TcpListener::bind(addr).await?;
		let (tx, rx) = tokio::sync::oneshot::channel::<()>();

		tokio::spawn(async {
			axum::serve(listener, router.into_make_service())
				.with_graceful_shutdown(async {
					rx.await.ok();
				})
				.await
				.expect("should start server")
		});

		self.exit_signal = Some(tx);

		Ok(())
	}

	pub async fn stop(&mut self) {
		if self.exit_signal.is_none() {
			return;
		}

		log::info!("stopping server");

		self
			.exit_signal
			.take()
			.expect("should have exit signal")
			.send(())
			.expect("should habe send exit signal");
	}

	fn add_tile_sources_to_app(&self, mut app: Router) -> Router {
		for tile_source in self.tile_sources.iter() {
			let route = tile_source.prefix.join_as_string("*path");

			let tile_app = Router::new()
				.route(&route, get(serve_tile))
				.with_state((tile_source.clone(), self.use_best_compression));

			app = app.merge(tile_app);

			async fn serve_tile(
				uri: Uri,
				headers: HeaderMap,
				State((tile_source, use_best_compression)): State<(TileSource, bool)>,
			) -> Response<Body> {
				let path = Url::new(uri.path());

				log::debug!("handle tile request: {path}");

				let mut target_compressions = get_encoding(headers);
				if !use_best_compression {
					target_compressions.set_fast_compression();
				}

				let response = tile_source
					.get_data(
						&path
							.strip_prefix(&tile_source.prefix)
							.expect("should start with prefix"),
						&target_compressions,
					)
					.await;

				if let Some(response) = response {
					log::info!("send response to tile request: {path}");
					ok_data(response, target_compressions)
				} else {
					log::warn!("send 404 to tile request: {path}");
					ok_not_found()
				}
			}
		}

		app
	}

	fn add_static_sources_to_app(&self, app: Router) -> Router {
		let static_app = Router::new()
			.fallback(get(serve_static))
			.with_state((self.static_sources.clone(), self.use_best_compression));

		return app.merge(static_app);

		async fn serve_static(
			uri: Uri,
			headers: HeaderMap,
			State((sources, use_best_compression)): State<(Vec<StaticSource>, bool)>,
		) -> Response<Body> {
			let mut url = Url::new(uri.path());

			log::debug!("handle static request: {url}");

			if url.is_dir() {
				url.push("index.html");
			}

			let mut target_compressions = get_encoding(headers);
			if !use_best_compression {
				target_compressions.set_fast_compression();
			}

			for source in sources.iter() {
				if let Some(result) = source.get_data(&url, &target_compressions) {
					log::info!("send response to static request: {url}");
					return ok_data(result, target_compressions);
				}
			}

			log::warn!("send 404 to static request: {url}");
			ok_not_found()
		}
	}

	async fn add_api_to_app(&self, app: Router) -> Result<Router> {
		let mut api_app = Router::new();
		api_app = api_app.route(
			"/api/status",
			get(|| async { ok_json("{\"status\":\"ready\"}") }),
		);

		let mut objects: Vec<String> = Vec::new();
		for tile_source in self.tile_sources.iter() {
			let id = &tile_source.id;
			let object = format!(
				"{{\"url\":\"{}\",\"id\":\"{}\",\"container\":{}}}",
				tile_source.prefix, id, tile_source.json_info
			);
			objects.push(object.clone());
			api_app = api_app.route(
				&format!("/api/source/{id}"),
				get(|| async move { ok_json(&object) }),
			);
		}
		let tile_sources_json = "[".to_owned() + &objects.join(",") + "]";
		api_app = api_app.route(
			"/api/sources",
			get(|| async move { ok_json(&tile_sources_json) }),
		);

		let tiles_index_json: String = format!(
			"[{}]",
			self
				.tile_sources
				.iter()
				.map(|s| format!("\"{}\"", s.id))
				.collect::<Vec<String>>()
				.join(","),
		);

		api_app = api_app.route(
			"/tiles/index.json",
			get(|| async move { ok_json(&tiles_index_json) }),
		);

		Ok(app.merge(api_app))
	}

	pub async fn get_url_mapping(&self) -> Vec<(String, String)> {
		let mut result = Vec::new();
		for tile_source in self.tile_sources.iter() {
			let id = tile_source.get_id().await;
			result.push((tile_source.prefix.as_string(), id.to_owned()))
		}
		result
	}
}

fn ok_not_found() -> Response<Body> {
	Response::builder()
		.status(404)
		.body(Body::from("Not Found"))
		.expect("should have build a body")
}

fn ok_data(result: SourceResponse, mut target_compressions: TargetCompression) -> Response<Body> {
	if matches!(
		result.mime.as_str(),
		"image/png" | "image/jpeg" | "image/webp" | "image/avif"
	) {
		target_compressions.set_incompressible();
	}

	let mut response = Response::builder()
		.status(200)
		.header(CONTENT_TYPE, result.mime)
		.header(CACHE_CONTROL, "public, max-age=2419200, no-transform")
		.header(VARY, "accept-encoding")
		.header(ACCESS_CONTROL_ALLOW_ORIGIN, "*");

	log::trace!(
		"optimize_compression from \"{}\" to {:?}",
		result.compression,
		target_compressions
	);
	let (blob, compression) =
		optimize_compression(result.blob, &result.compression, &target_compressions)
			.expect("should have optimized compression");

	use TileCompression::*;
	match compression {
		Uncompressed => {}
		Gzip => response = response.header(CONTENT_ENCODING, "gzip"),
		Brotli => response = response.header(CONTENT_ENCODING, "br"),
	}

	log::trace!("send repsonse using headers: {:?}", response.headers_ref());

	response
		.body(Body::from(blob.into_vec()))
		.expect("should have build a body")
}

fn ok_json(message: &str) -> Response<Body> {
	ok_data(
		SourceResponse {
			blob: Blob::from(message),
			compression: TileCompression::Uncompressed,
			mime: String::from("application/json"),
		},
		TargetCompression::from_none(),
	)
}

fn get_encoding(headers: HeaderMap) -> TargetCompression {
	let mut encoding_set: TargetCompression = TargetCompression::from_none();
	let encoding_option = headers.get(ACCEPT_ENCODING);
	if let Some(encoding) = encoding_option {
		let encoding_string = encoding.to_str().unwrap_or("");

		if encoding_string.contains("gzip") {
			encoding_set.insert(TileCompression::Gzip);
		}
		if encoding_string.contains("br") {
			encoding_set.insert(TileCompression::Brotli);
		}
	}
	encoding_set
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{
		container::{MockTilesReader, MockTilesReaderProfile},
		types::TileCompression::*,
	};
	use axum::http::{header::ACCEPT_ENCODING, HeaderMap};
	use enumset::{enum_set, EnumSet};

	const IP: &str = "127.0.0.1";

	#[test]
	fn test_get_encoding() {
		let test = |encoding: &str, comp0: EnumSet<TileCompression>| {
			let mut map = HeaderMap::new();
			if encoding != "NONE" {
				map.insert(ACCEPT_ENCODING, encoding.parse().unwrap());
			}
			let comp0 = TargetCompression::from_set(comp0);
			let comp = get_encoding(map);
			assert_eq!(comp, comp0);
		};

		test("NONE", enum_set!(Uncompressed));
		test("", enum_set!(Uncompressed));
		test("*", enum_set!(Uncompressed));
		test("br", enum_set!(Uncompressed | Brotli));
		test(
			"br;q=1.0, gzip;q=0.8, *;q=0.1",
			enum_set!(Uncompressed | Brotli | Gzip),
		);
		test("compress", enum_set!(Uncompressed));
		test("compress, gzip", enum_set!(Uncompressed | Gzip));
		test("compress;q=0.5, gzip;q=1.0", enum_set!(Uncompressed | Gzip));
		test("deflate", enum_set!(Uncompressed));
		test(
			"deflate, gzip;q=1.0, *;q=0.5",
			enum_set!(Uncompressed | Gzip),
		);
		test("gzip", enum_set!(Uncompressed | Gzip));
		test(
			"gzip, compress, br",
			enum_set!(Uncompressed | Brotli | Gzip),
		);
		test(
			"gzip, deflate, br;q=1.0, identity;q=0.5, *;q=0.25",
			enum_set!(Uncompressed | Brotli | Gzip),
		);
		test(
			"gzip;q=1.0, identity; q=0.5, *;q=0",
			enum_set!(Uncompressed | Gzip),
		);
		test("identity", enum_set!(Uncompressed));
	}

	#[tokio::test]
	async fn server() {
		async fn get(path: &str) -> String {
			reqwest::get(format!("http://{IP}:50001/{path}"))
				.await
				.expect("should have made a get request")
				.text()
				.await
				.expect("should have returned text")
		}

		let mut server = TileServer::new(IP, 50001, true, true);

		let reader = MockTilesReader::new_mock_profile(MockTilesReaderProfile::Pbf)
			.unwrap()
			.boxed();
		server.add_tile_source("cheese", reader).unwrap();

		server.start().await.unwrap();

		const JSON:&str = "{\"url\":\"/tiles/cheese/\",\"id\":\"cheese\",\"container\":{\"type\":\"dummy_container\",\"format\":\"pbf\",\"compression\":\"gzip\",\"zoom_min\":0,\"zoom_max\":4,\"bbox\":[-180,-85.05112877980659,180,85.05112877980659]}}";
		assert_eq!(get("api/status").await, "{\"status\":\"ready\"}");
		assert_eq!(get("api/sources").await, format!("[{JSON}]"));
		assert_eq!(get("api/source/burger").await, "Not Found");
		assert_eq!(get("api/source/dummy_name").await, "Not Found");
		assert_eq!(get("api/source/cheese").await, JSON);
		assert_eq!(get("tiles/cheese/brum.json").await, "Not Found");
		assert_eq!(get("tiles/cheese/meta.json").await, "{\"type\":\"dummy\"}");
		assert_eq!(get("tiles/cheese/tiles.json").await, "{\"type\":\"dummy\"}");
		assert!(get("tiles/cheese/0/0/0.png")
			.await
			.starts_with("\u{1a}4\n\u{5}ocean"));
		assert_eq!(get("tiles/index.json").await, "[\"cheese\"]");
		assert_eq!(get("status").await, "ready!");

		server.stop().await;
	}

	#[tokio::test]
	#[should_panic]
	async fn same_prefix_twice() {
		let mut server = TileServer::new(IP, 50002, true, true);

		let reader = MockTilesReader::new_mock_profile(MockTilesReaderProfile::Png)
			.unwrap()
			.boxed();
		server.add_tile_source("cheese", reader).unwrap();

		let reader = MockTilesReader::new_mock_profile(MockTilesReaderProfile::Pbf)
			.unwrap()
			.boxed();
		server.add_tile_source("cheese", reader).unwrap();
	}

	#[tokio::test]
	async fn tile_server_new() {
		let mut server = TileServer::new(IP, 50003, true, true);
		assert_eq!(server.ip, IP);
		assert_eq!(server.port, 50003);
		assert_eq!(server.tile_sources.len(), 0);
		assert_eq!(server.static_sources.len(), 0);
		assert!(server.exit_signal.is_none());

		assert!(server.start().await.is_ok());

		server.stop().await; // No assertion here as it's void
	}

	#[test]
	fn tile_server_add_tile_source() {
		let mut server = TileServer::new(IP, 50004, true, true);
		assert_eq!(server.ip, IP);
		assert_eq!(server.port, 50004);

		let reader = MockTilesReader::new_mock_profile(MockTilesReaderProfile::Pbf)
			.unwrap()
			.boxed();
		server.add_tile_source("cheese", reader).unwrap();

		assert_eq!(server.tile_sources.len(), 1);
		assert_eq!(server.tile_sources[0].prefix.str, "/tiles/cheese/");
	}

	#[tokio::test]
	async fn tile_server_iter_url_mapping() {
		let mut server = TileServer::new(IP, 50005, true, true);
		assert_eq!(server.ip, IP);
		assert_eq!(server.port, 50005);

		let reader = MockTilesReader::new_mock_profile(MockTilesReaderProfile::Pbf)
			.unwrap()
			.boxed();
		server.add_tile_source("cheese", reader).unwrap();

		let mappings: Vec<(String, String)> = server.get_url_mapping().await;
		assert_eq!(mappings.len(), 1);
		assert_eq!(mappings[0].0, "/tiles/cheese/");
		assert_eq!(mappings[0].1, "dummy_name");
	}
}
