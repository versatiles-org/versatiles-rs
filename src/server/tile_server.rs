use super::{ServerSource, ServerSourceTrait};
use crate::shared::{Blob, Compression, Result};
use axum::{
	body::{Bytes, Full},
	extract::{Path, State},
	http::{
		header::{ACCEPT_ENCODING, CACHE_CONTROL, CONTENT_ENCODING, CONTENT_TYPE},
		HeaderMap, Uri,
	},
	response::Response,
	routing::get,
	Router, Server,
};
use enumset::{enum_set, EnumSet};
use futures::{executor::block_on, lock::Mutex};
use std::sync::Arc;
use tokio::sync::oneshot::Sender;

struct TileSource {
	prefix: String,
	source: ServerSource,
}

pub struct TileServer {
	ip: String,
	port: u16,
	tile_sources: Vec<TileSource>,
	static_sources: Vec<ServerSource>,
	exit_signal: Option<Sender<()>>,
}

impl TileServer {
	pub fn new(ip: &str, port: u16) -> TileServer {
		TileServer {
			ip: ip.to_owned(),
			port,
			tile_sources: Vec::new(),
			static_sources: Vec::new(),
			exit_signal: None,
		}
	}

	pub fn add_tile_source(&mut self, url_prefix: &str, tile_source: Box<dyn ServerSourceTrait>) {
		log::debug!("add source: prefix='{}', source={:?}", url_prefix, tile_source);

		let mut prefix = url_prefix.trim().to_owned();
		if !prefix.starts_with('/') {
			prefix = "/".to_owned() + &prefix;
		}
		if !prefix.ends_with('/') {
			prefix += "/";
		}

		for other_tile_source in self.tile_sources.iter() {
			if other_tile_source.prefix.starts_with(&prefix) || prefix.starts_with(&other_tile_source.prefix) {
				panic!(
					"multiple sources with the prefix '{}' and '{}' are defined",
					prefix, other_tile_source.prefix
				);
			};
		}

		self.tile_sources.push(TileSource {
			prefix,
			source: Arc::new(Mutex::new(tile_source)),
		});
	}

	pub fn add_static_source(&mut self, source: Box<dyn ServerSourceTrait>) {
		log::debug!("set static: source={:?}", source);
		self.static_sources.push(Arc::new(Mutex::new(source)));
	}

	pub async fn start(&mut self) -> Result<()> {
		if self.exit_signal.is_some() {
			self.stop().await
		}

		log::debug!("starting server");

		// Initialize App
		let mut app = Router::new().route("/status", get(|| async { "ready!" }));

		app = self.add_tile_sources_to_app(app);
		app = self.add_api_to_app(app)?;
		app = self.add_static_sources_to_app(app);

		let addr = format!("{}:{}", self.ip, self.port);
		println!("server starts listening on {}", addr);

		let server = Server::bind(&addr.parse()?).serve(app.into_make_service());

		let (tx, rx) = tokio::sync::oneshot::channel::<()>();
		let graceful = server.with_graceful_shutdown(async {
			rx.await.ok();
		});

		self.exit_signal = Some(tx);

		tokio::spawn(async move {
			if let Err(e) = graceful.await {
				eprintln!("server error: {}", e);
			}
		});

		Ok(())
	}

	pub async fn stop(&mut self) {
		if self.exit_signal.is_none() {
			return;
		}

		log::debug!("stopping server");

		self.exit_signal.take().unwrap().send(()).unwrap();
	}

	fn add_tile_sources_to_app(&self, mut app: Router) -> Router {
		for tile_source in self.tile_sources.iter() {
			let route = tile_source.prefix.to_owned() + "*path";

			let tile_app = Router::new()
				.route(&route, get(serve_tile))
				.with_state(tile_source.source.clone());
			app = app.merge(tile_app);

			async fn serve_tile(
				Path(path): Path<String>, headers: HeaderMap, State(source_mutex): State<ServerSource>,
			) -> Response<Full<Bytes>> {
				let sub_path: Vec<&str> = path.split('/').collect();
				let mut source = source_mutex.lock().await;
				let response = source.get_data(&sub_path, get_encoding(headers)).await;
				drop(source);
				response
			}
		}

		app
	}

	fn add_static_sources_to_app(&self, app: Router) -> Router {
		let static_app = Router::new()
			.fallback(get(serve_static))
			.with_state(self.static_sources.clone());

		return app.merge(static_app);

		async fn serve_static(
			uri: Uri, headers: HeaderMap, State(sources): State<Vec<ServerSource>>,
		) -> Response<Full<Bytes>> {
			let mut path_vec: Vec<&str> = uri.path().split('/').skip(1).collect();

			if let Some(last) = path_vec.last_mut() {
				if last.is_empty() {
					*last = "index.html";
				}
			}

			let path_slice = path_vec.as_slice();
			let encoding_set = get_encoding(headers);

			for source in sources.iter() {
				let mut source = source.lock().await;
				let response = source.get_data(path_slice, encoding_set).await;
				drop(source);
				if response.status() == 200 {
					return response;
				}
			}

			ok_not_found()
		}
	}

	fn add_api_to_app(&self, app: Router) -> Result<Router> {
		let mut tile_sources_json_lines: Vec<String> = Vec::new();
		for tile_source in self.tile_sources.iter() {
			let source = block_on(tile_source.source.lock());
			tile_sources_json_lines.push(format!(
				"{{ \"url\":\"{}\", \"name\":\"{}\", \"info\":{} }}",
				tile_source.prefix,
				source.get_name()?,
				source.get_info_as_json()?
			));
			drop(source);
		}
		let tile_sources_json: String = "[\n\t".to_owned() + &tile_sources_json_lines.join(",\n\t") + "\n]";

		let api_app = Router::new()
			.route(
				"/api/status.json",
				get(|| async {
					ok_data(
						Blob::from("{\"status\":\"ready\"}"),
						&Compression::None,
						"application/json",
					)
				}),
			)
			.route(
				"/api/tiles.json",
				get(|| async move { ok_data(Blob::from(&tile_sources_json), &Compression::None, "application/json") }),
			);

		Ok(app.merge(api_app))
	}

	pub fn iter_url_mapping(&self) -> impl Iterator<Item = (String, String)> + '_ {
		self.tile_sources.iter().map(|tile_source| {
			let source = block_on(tile_source.source.lock());
			(
				tile_source.prefix.to_owned(),
				source.get_name().unwrap_or(String::from("???")),
			)
		})
	}
}

pub fn ok_not_found() -> Response<Full<Bytes>> {
	Response::builder().status(404).body(Full::from("Not Found")).unwrap()
}

pub fn ok_data(data: Blob, compression: &Compression, mime: &str) -> Response<Full<Bytes>> {
	let mut response = Response::builder()
		.status(200)
		.header(CONTENT_TYPE, mime)
		.header(CACHE_CONTROL, "public");

	match compression {
		Compression::None => {}
		Compression::Gzip => response = response.header(CONTENT_ENCODING, "gzip"),
		Compression::Brotli => response = response.header(CONTENT_ENCODING, "br"),
	}

	response.body(Full::from(data.as_vec())).unwrap()
}

pub fn guess_mime(path: &std::path::Path) -> String {
	let mime = mime_guess::from_path(path).first_or_octet_stream();
	return mime.essence_str().to_owned();
}

fn get_encoding(headers: HeaderMap) -> EnumSet<Compression> {
	let mut encoding_set: EnumSet<Compression> = enum_set!(Compression::None);
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
	use crate::{
		containers::dummy,
		server::source::TileContainer,
		shared::Compression::{self, *},
	};
	use axum::http::{header::ACCEPT_ENCODING, HeaderMap};
	use enumset::{enum_set, EnumSet};
	use std::path::Path;

	const IP: &str = "127.0.0.1";
	const PORT: u16 = 3000;

	#[test]
	fn test_get_encoding() {
		let test = |encoding: &str, comp0: EnumSet<Compression>| {
			let mut map = HeaderMap::new();
			if encoding != "NONE" {
				map.insert(ACCEPT_ENCODING, encoding.parse().unwrap());
			}
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

		test("fluffy.css", "text/css");
		test("fluffy.gif", "image/gif");
		test("fluffy.htm", "text/html");
		test("fluffy.html", "text/html");
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
			reqwest::get(format!("http://{IP}:{PORT}/{path}"))
				.await
				.unwrap()
				.text()
				.await
				.unwrap()
		}

		let mut server = TileServer::new(IP, PORT);

		let reader = dummy::TileReader::new_dummy(dummy::ReaderProfile::PbfFast, 8);
		let source = TileContainer::from(reader).unwrap();
		server.add_tile_source("cheese", source);

		let reader = dummy::TileReader::new_dummy(dummy::ReaderProfile::PbfFast, 8);
		let source = TileContainer::from(reader).unwrap();
		server.add_static_source(source);

		server.start().await.unwrap();

		assert_eq!(get("api/status.json").await, "{\"status\":\"ready\"}");
		assert_eq!(get("api/tiles.json").await, "[\n\t{ \"url\":\"/cheese/\", \"name\":\"dummy name\", \"info\":{ \"container\":\"dummy container\", \"format\":\"pbf\", \"compression\":\"gzip\", \"zoom_min\":0, \"zoom_max\":8, \"bbox\":[-180.0, -85.05113, 180.0, 85.05112] } }\n]");
		assert!(get("cheese/0/0/0.png").await.starts_with("\u{1a}4\n\u{5}ocean"));
		assert_eq!(get("cheese/meta.json").await, "dummy meta data");
		assert_eq!(get("cheese/tiles.json").await, "dummy meta data");
		assert_eq!(get("cheese/brum.json").await, "Not Found");
		assert_eq!(get("status").await, "ready!");

		server.stop().await;
	}

	#[tokio::test]
	#[should_panic]
	async fn panic() {
		let mut server = TileServer::new(IP, PORT);

		let reader = dummy::TileReader::new_dummy(dummy::ReaderProfile::PngFast, 8);
		let source = TileContainer::from(reader).unwrap();
		server.add_tile_source("cheese", source);

		let reader = dummy::TileReader::new_dummy(dummy::ReaderProfile::PbfFast, 8);
		let source = TileContainer::from(reader).unwrap();
		server.add_tile_source("cheese", source);
	}

	#[test]
	fn tile_server_new() {
		let ip = "127.0.0.1";
		let port = 8080;

		let server = TileServer::new(ip, port);
		assert_eq!(server.ip, ip);
		assert_eq!(server.port, port);
		assert_eq!(server.tile_sources.len(), 0);
		assert_eq!(server.static_sources.len(), 0);
		assert!(server.exit_signal.is_none());
	}

	#[test]
	fn tile_server_add_tile_source() {
		let mut server = TileServer::new(IP, PORT);

		let reader = dummy::TileReader::new_dummy(dummy::ReaderProfile::PbfFast, 8);
		let source = TileContainer::from(reader).unwrap();
		server.add_tile_source("cheese", source);

		assert_eq!(server.tile_sources.len(), 1);
		assert_eq!(server.tile_sources[0].prefix, "/cheese/");
	}

	#[test]
	fn tile_server_add_static_source() {
		let mut server = TileServer::new(IP, PORT);

		let reader = dummy::TileReader::new_dummy(dummy::ReaderProfile::PbfFast, 8);
		let source = TileContainer::from(reader).unwrap();
		server.add_static_source(source);

		assert_eq!(server.static_sources.len(), 1);
	}

	#[test]
	fn tile_server_iter_url_mapping() {
		let mut server = TileServer::new(IP, PORT);

		let reader = dummy::TileReader::new_dummy(dummy::ReaderProfile::PbfFast, 8);
		let source = TileContainer::from(reader).unwrap();
		server.add_tile_source("cheese", source);

		let mappings: Vec<(String, String)> = server.iter_url_mapping().collect();
		assert_eq!(mappings.len(), 1);
		assert_eq!(mappings[0].0, "/cheese/");
		assert_eq!(mappings[0].1, "dummy name");
	}
}
