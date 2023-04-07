use crate::ServerSourceTrait;
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
use std::sync::Arc;
use tokio::sync::oneshot::Sender;
use versatiles_shared::{Blob, Compression};

struct TileSource {
	prefix: String,
	source: Arc<Box<dyn ServerSourceTrait>>,
}

pub struct TileServer {
	ip: String,
	port: u16,
	tile_sources: Vec<TileSource>,
	static_sources: Vec<Arc<Box<dyn ServerSourceTrait>>>,
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
			source: Arc::new(tile_source),
		});
	}

	pub fn add_static_source(&mut self, source: Box<dyn ServerSourceTrait>) {
		log::debug!("set static: source={:?}", source);
		self.static_sources.push(Arc::new(source));
	}

	pub async fn start(&mut self) {
		if self.exit_signal.is_some() {
			self.stop().await
		}

		log::debug!("starting server");

		// Initialize App
		let mut app = Router::new().route("/status", get(|| async { "ready!" }));

		app = self.add_tile_sources_to_app(app);
		app = self.add_api_to_app(app);
		app = self.add_static_sources_to_app(app);

		let addr = format!("{}:{}", self.ip, self.port);
		println!("server starts listening on {}", addr);

		let server = Server::bind(&addr.parse().unwrap()).serve(app.into_make_service());

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
			let source = tile_source.source.clone();

			let tile_app = Router::new().route(&route, get(serve_tile)).with_state(source);
			app = app.merge(tile_app);

			async fn serve_tile(
				Path(path): Path<String>, headers: HeaderMap, State(source): State<Arc<Box<dyn ServerSourceTrait>>>,
			) -> Response<Full<Bytes>> {
				let sub_path: Vec<&str> = path.split('/').collect();
				source.get_data(&sub_path, get_encoding(headers)).await
			}
		}

		app
	}

	fn add_static_sources_to_app(&self, app: Router) -> Router {
		let sources = self.static_sources.clone();

		let static_app = Router::new().fallback(get(serve_static)).with_state(sources);

		return app.merge(static_app);

		async fn serve_static(
			uri: Uri, headers: HeaderMap, State(sources): State<Vec<Arc<Box<dyn ServerSourceTrait>>>>,
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
				let response = source.get_data(path_slice, encoding_set).await;
				if response.status() == 200 {
					return response;
				}
			}

			ok_not_found()
		}
	}

	fn add_api_to_app(&self, app: Router) -> Router {
		let mut tile_sources_json_lines: Vec<String> = Vec::new();
		for tile_source in self.tile_sources.iter() {
			tile_sources_json_lines.push(format!(
				"{{ \"url\":\"{}\", \"name\":\"{}\", \"info\":{} }}",
				tile_source.prefix,
				tile_source.source.get_name(),
				tile_source.source.get_info_as_json()
			));
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

		app.merge(api_app)
	}

	pub fn iter_url_mapping(&self) -> impl Iterator<Item = (String, String)> + '_ {
		self
			.tile_sources
			.iter()
			.map(|tile_source| (tile_source.prefix.to_owned(), tile_source.source.get_name()))
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
	use crate::source::TileContainer;
	use axum::http::{header::ACCEPT_ENCODING, HeaderMap};
	use enumset::{enum_set, EnumSet};
	use std::path::Path;
	use versatiles_container::dummy;
	use versatiles_shared::Compression;
	use versatiles_shared::Compression::*;

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
	async fn test_server() {
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
		let source = TileContainer::from(reader);
		server.add_tile_source("cheese", source);

		let reader = dummy::TileReader::new_dummy(dummy::ReaderProfile::PbfFast, 8);
		let source = TileContainer::from(reader);
		server.add_static_source(source);

		server.start().await;

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
	async fn test_panic() {
		let mut server = TileServer::new(IP, PORT);

		let reader = dummy::TileReader::new_dummy(dummy::ReaderProfile::PngFast, 8);
		let source = TileContainer::from(reader);
		server.add_tile_source("cheese", source);

		let reader = dummy::TileReader::new_dummy(dummy::ReaderProfile::PbfFast, 8);
		let source = TileContainer::from(reader);
		server.add_tile_source("cheese", source);
	}
}
