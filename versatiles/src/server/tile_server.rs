use super::sources::{SourceResponse, StaticSource, TileSource};
use anyhow::Result;
use axum::{
	body::Body,
	extract::{Path, State},
	http::{
		header::{ACCEPT_ENCODING, CACHE_CONTROL, CONTENT_ENCODING, CONTENT_TYPE},
		HeaderMap, Uri,
	},
	response::Response,
	routing::get,
	Router,
};
use hyper::header::{ACCESS_CONTROL_ALLOW_ORIGIN, VARY};
use tokio::sync::oneshot::Sender;
use versatiles_lib::{
	containers::TileReaderBox,
	create_error,
	shared::{optimize_compression, Blob, Compression, TargetCompression},
};

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

	pub fn add_tile_source(&mut self, prefix: &str, id: &str, reader: TileReaderBox) -> Result<()> {
		log::info!("add source: prefix='{}', source={:?}", prefix, reader);

		let mut prefix: String = prefix.trim().to_owned();
		if !prefix.starts_with('/') {
			prefix = "/".to_owned() + &prefix;
		}
		if !prefix.ends_with('/') {
			prefix += "/";
		}

		for other_tile_source in self.tile_sources.iter() {
			if other_tile_source.prefix.starts_with(&prefix) || prefix.starts_with(&other_tile_source.prefix) {
				return create_error!(
					"multiple sources with the prefix '{}' and '{}' are defined",
					prefix,
					other_tile_source.prefix
				);
			};
		}

		self.tile_sources.push(TileSource::from(reader, id, &prefix)?);

		Ok(())
	}

	pub fn add_static_source(&mut self, filename: &str, path: &str) -> Result<()> {
		log::info!("add static: {filename}");
		self.static_sources.push(StaticSource::new(filename, path)?);
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
				.unwrap()
		});

		self.exit_signal = Some(tx);

		Ok(())
	}

	pub async fn stop(&mut self) {
		if self.exit_signal.is_none() {
			return;
		}

		log::info!("stopping server");

		self.exit_signal.take().unwrap().send(()).unwrap();
	}

	fn add_tile_sources_to_app(&self, mut app: Router) -> Router {
		for tile_source in self.tile_sources.iter() {
			let route = tile_source.prefix.to_owned() + "*path";

			let tile_app = Router::new()
				.route(&route, get(serve_tile))
				.with_state((tile_source.clone(), self.use_best_compression));

			app = app.merge(tile_app);

			async fn serve_tile(
				Path(path): Path<String>, headers: HeaderMap,
				State((tile_source, best_compression)): State<(TileSource, bool)>,
			) -> Response<Body> {
				let sub_path: Vec<&str> = path.split('/').collect();

				let mut target_compressions = get_encoding(headers);
				target_compressions.set_best_compression(best_compression);

				let response = tile_source.get_data(&sub_path, &target_compressions).await;

				if let Some(response) = response {
					log::warn!("{}: {} found", tile_source.prefix, path);
					ok_data(response, target_compressions)
				} else {
					log::warn!("{}: {} not found", tile_source.prefix, path);
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
			uri: Uri, headers: HeaderMap, State((sources, best_compression)): State<(Vec<StaticSource>, bool)>,
		) -> Response<Body> {
			let path = uri.path();
			let mut path_vec: Vec<&str> = path.split('/').skip(1).collect();

			if let Some(last) = path_vec.last_mut() {
				if last.is_empty() {
					*last = "index.html";
				}
			}

			let path_slice = path_vec.as_slice();
			let mut compressions = get_encoding(headers);
			compressions.set_best_compression(best_compression);

			for source in sources.iter() {
				if let Some(result) = source.get_data(path_slice, &compressions) {
					return ok_data(result, compressions);
				}
			}

			ok_not_found()
		}
	}

	async fn add_api_to_app(&self, app: Router) -> Result<Router> {
		let mut api_app = Router::new();
		api_app = api_app.route("/api/status", get(|| async { ok_json("{\"status\":\"ready\"}") }));

		let mut objects: Vec<String> = Vec::new();
		for tile_source in self.tile_sources.iter() {
			let object = format!(
				"{{\"url\":\"{}\",\"id\":\"{}\",\"container\":{}}}",
				tile_source.prefix, tile_source.id, tile_source.json_info
			);
			objects.push(object.clone());
			api_app = api_app.route(
				&format!("/api/source/{}", tile_source.id),
				get(|| async move { ok_json(&object) }),
			);
		}
		let tile_sources_json: String = "[".to_owned() + &objects.join(",") + "]";

		api_app = api_app.route("/api/sources", get(|| async move { ok_json(&tile_sources_json) }));

		Ok(app.merge(api_app))
	}

	pub async fn get_url_mapping(&self) -> Vec<(String, String)> {
		let mut result = Vec::new();
		for tile_source in self.tile_sources.iter() {
			result.push((tile_source.prefix.to_owned(), tile_source.id.to_owned()))
		}
		result
	}
}

fn ok_not_found() -> Response<Body> {
	Response::builder().status(404).body(Body::from("Not Found")).unwrap()
}

fn ok_data(result: SourceResponse, target_compressions: TargetCompression) -> Response<Body> {
	let is_incompressible = matches!(
		result.mime.as_str(),
		"image/png" | "image/jpeg" | "image/webp" | "image/avif"
	);

	let mut response = Response::builder()
		.status(200)
		.header(CONTENT_TYPE, result.mime)
		.header(CACHE_CONTROL, "public, max-age=2419200, no-transform")
		.header(VARY, "accept-encoding")
		.header(ACCESS_CONTROL_ALLOW_ORIGIN, "*");

	let (blob, compression) = if is_incompressible {
		(result.blob, result.compression)
	} else {
		optimize_compression(result.blob, &result.compression, target_compressions).unwrap()
	};

	match compression {
		Compression::None => {}
		Compression::Gzip => response = response.header(CONTENT_ENCODING, "gzip"),
		Compression::Brotli => response = response.header(CONTENT_ENCODING, "br"),
	}

	response.body(Body::from(blob.as_vec())).unwrap()
}

fn ok_json(message: &str) -> Response<Body> {
	ok_data(
		SourceResponse {
			blob: Blob::from(message),
			compression: Compression::None,
			mime: String::from("application/json"),
		},
		TargetCompression::from_none(),
	)
}

pub fn guess_mime(path: &std::path::Path) -> String {
	let mime = mime_guess::from_path(path)
		.first_or_octet_stream()
		.essence_str()
		.to_owned();
	if mime.starts_with("text/") {
		format!("{mime}; charset=utf-8")
	} else {
		mime
	}
}

fn get_encoding(headers: HeaderMap) -> TargetCompression {
	let mut encoding_set: TargetCompression = TargetCompression::from_none();
	let encoding_option = headers.get(ACCEPT_ENCODING);
	if let Some(encoding) = encoding_option {
		let encoding_string = encoding.to_str().unwrap_or("");

		if encoding_string.contains("gzip") {
			encoding_set.insert(Compression::Gzip);
		}
		if encoding_string.contains("br") {
			encoding_set.insert(Compression::Brotli);
		}
	}
	encoding_set
}

#[cfg(test)]
mod tests {
	use super::{get_encoding, guess_mime, TileServer};
	use axum::http::{header::ACCEPT_ENCODING, HeaderMap};
	use enumset::{enum_set, EnumSet};
	use std::path::Path;
	use versatiles_lib::{
		containers::mock,
		shared::{
			Compression::{self, *},
			TargetCompression,
		},
	};

	const IP: &str = "127.0.0.1";

	#[test]
	fn test_get_encoding() {
		let test = |encoding: &str, comp0: EnumSet<Compression>| {
			let mut map = HeaderMap::new();
			if encoding != "NONE" {
				map.insert(ACCEPT_ENCODING, encoding.parse().unwrap());
			}
			let comp0 = TargetCompression::from_set(comp0);
			let comp = get_encoding(map);
			assert_eq!(comp, comp0);
		};

		test("NONE", enum_set!(None));
		test("", enum_set!(None));
		test("*", enum_set!(None));
		test("br", enum_set!(None | Brotli));
		test("br;q=1.0, gzip;q=0.8, *;q=0.1", enum_set!(None | Brotli | Gzip));
		test("compress", enum_set!(None));
		test("compress, gzip", enum_set!(None | Gzip));
		test("compress;q=0.5, gzip;q=1.0", enum_set!(None | Gzip));
		test("deflate", enum_set!(None));
		test("deflate, gzip;q=1.0, *;q=0.5", enum_set!(None | Gzip));
		test("gzip", enum_set!(None | Gzip));
		test("gzip, compress, br", enum_set!(None | Brotli | Gzip));
		test(
			"gzip, deflate, br;q=1.0, identity;q=0.5, *;q=0.25",
			enum_set!(None | Brotli | Gzip),
		);
		test("gzip;q=1.0, identity; q=0.5, *;q=0", enum_set!(None | Gzip));
		test("identity", enum_set!(None));
	}

	#[test]
	fn test_guess_mime() {
		let test = |path: &str, mime: &str| {
			assert_eq!(guess_mime(Path::new(path)), mime);
		};

		test("fluffy.css", "text/css; charset=utf-8");
		test("fluffy.gif", "image/gif");
		test("fluffy.htm", "text/html; charset=utf-8");
		test("fluffy.html", "text/html; charset=utf-8");
		test("fluffy.jpeg", "image/jpeg");
		test("fluffy.jpg", "image/jpeg");
		test("fluffy.js", "application/javascript");
		test("fluffy.json", "application/json");
		test("fluffy.pbf", "application/octet-stream");
		test("fluffy.png", "image/png");
		test("fluffy.svg", "image/svg+xml");
	}

	#[tokio::test]
	async fn server() {
		async fn get(path: &str) -> String {
			reqwest::get(format!("http://{IP}:50001/{path}"))
				.await
				.unwrap()
				.text()
				.await
				.unwrap()
		}

		let mut server = TileServer::new(IP, 50001, true, true);

		let reader = mock::TileReader::new_mock(mock::ReaderProfile::PBF, 8);
		server.add_tile_source("cheese", "burger", reader).unwrap();

		server.start().await.unwrap();

		const JSON:&str = "{\"url\":\"/cheese/\",\"id\":\"burger\",\"container\":{\"type\":\"dummy container\",\"format\":\"pbf\",\"compression\":\"gzip\",\"zoom_min\":0,\"zoom_max\":8,\"bbox\":[-180,-85.05112877980659,180,85.05112877980659]}}";
		assert_eq!(get("api/status").await, "{\"status\":\"ready\"}");
		assert_eq!(get("api/sources").await, format!("[{JSON}]"));
		assert_eq!(get("api/source/cheese").await, "Not Found");
		assert_eq!(get("api/source/burger").await, JSON);
		assert!(get("cheese/0/0/0.png").await.starts_with("\u{1a}4\n\u{5}ocean"));
		assert_eq!(get("cheese/meta.json").await, "dummy meta data");
		assert_eq!(get("cheese/tiles.json").await, "dummy meta data");
		assert_eq!(get("cheese/brum.json").await, "Not Found");
		assert_eq!(get("status").await, "ready!");

		server.stop().await;
	}

	#[tokio::test]
	#[should_panic]
	async fn same_prefix_twice() {
		let mut server = TileServer::new(IP, 50002, true, true);

		let reader = mock::TileReader::new_mock(mock::ReaderProfile::PNG, 8);
		server.add_tile_source("cheese", "soup", reader).unwrap();

		let reader = mock::TileReader::new_mock(mock::ReaderProfile::PBF, 8);
		server.add_tile_source("cheese", "sandwich", reader).unwrap();
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

		let reader = mock::TileReader::new_mock(mock::ReaderProfile::PBF, 8);
		server.add_tile_source("cheese", "pizza", reader).unwrap();

		assert_eq!(server.tile_sources.len(), 1);
		assert_eq!(server.tile_sources[0].prefix, "/cheese/");
	}

	#[tokio::test]
	async fn tile_server_iter_url_mapping() {
		let mut server = TileServer::new(IP, 50005, true, true);
		assert_eq!(server.ip, IP);
		assert_eq!(server.port, 50005);

		let reader = mock::TileReader::new_mock(mock::ReaderProfile::PBF, 8);
		server.add_tile_source("cheese", "cake", reader).unwrap();

		let mappings: Vec<(String, String)> = server.get_url_mapping().await;
		assert_eq!(mappings.len(), 1);
		assert_eq!(mappings[0].0, "/cheese/");
		assert_eq!(mappings[0].1, "cake");
	}
}
