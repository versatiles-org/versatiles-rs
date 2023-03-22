use super::traits::ServerSourceBox;
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
use versatiles_shared::{Blob, Precompression};

struct TileSource {
	prefix: String,
	source: Arc<ServerSourceBox>,
}

pub struct TileServer {
	ip: String,
	port: u16,
	tile_sources: Vec<TileSource>,
	static_sources: Vec<Arc<ServerSourceBox>>,
}

impl TileServer {
	pub fn new(ip: &str, port: u16) -> TileServer {
		TileServer {
			ip: ip.to_owned(),
			port,
			tile_sources: Vec::new(),
			static_sources: Vec::new(),
		}
	}

	pub fn add_tile_source(&mut self, url_prefix: String, tile_source: ServerSourceBox) {
		log::debug!("add source: prefix='{}', source={:?}", url_prefix, tile_source);

		let mut prefix = url_prefix;
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

	pub fn add_static_source(&mut self, source: ServerSourceBox) {
		log::debug!("set static: source={:?}", source);
		self.static_sources.push(Arc::new(source));
	}

	pub async fn start(&mut self) {
		log::debug!("starting server");

		// Initialize App
		let mut app = Router::new().route("/status", get(|| async { "ready!" }));

		app = self.add_tile_sources_to_app(app);
		app = self.add_api_to_app(app);
		app = self.add_static_sources_to_app(app);

		let addr = format!("{}:{}", self.ip, self.port);
		println!("server starts listening on {}", addr);

		Server::bind(&addr.parse().unwrap())
			.serve(app.into_make_service())
			.await
			.expect("server failed");
	}

	fn add_tile_sources_to_app(&self, mut app: Router) -> Router {
		for tile_source in self.tile_sources.iter() {
			let route = tile_source.prefix.to_owned() + "*path";
			let source = tile_source.source.clone();

			let tile_app = Router::new().route(&route, get(serve_tile)).with_state(source);
			app = app.merge(tile_app);

			async fn serve_tile(
				Path(path): Path<String>, headers: HeaderMap, State(source): State<Arc<ServerSourceBox>>,
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
			uri: Uri, headers: HeaderMap, State(sources): State<Vec<Arc<ServerSourceBox>>>,
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
						Blob::from_string("{{\"status\":\"ready\"}}"),
						&Precompression::Uncompressed,
						"application/json",
					)
				}),
			)
			.route(
				"/api/tiles.json",
				get(|| async move {
					ok_data(
						Blob::from_string(&tile_sources_json),
						&Precompression::Uncompressed,
						"application/json",
					)
				}),
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

pub fn ok_data(data: Blob, precompression: &Precompression, mime: &str) -> Response<Full<Bytes>> {
	let mut response = Response::builder()
		.status(200)
		.header(CONTENT_TYPE, mime)
		.header(CACHE_CONTROL, "public");

	match precompression {
		Precompression::Uncompressed => {}
		Precompression::Gzip => response = response.header(CONTENT_ENCODING, "gzip"),
		Precompression::Brotli => response = response.header(CONTENT_ENCODING, "br"),
	}

	response.body(Full::from(data.as_vec())).unwrap()
}

pub fn guess_mime(path: &std::path::Path) -> String {
	let mime = mime_guess::from_path(path).first_or_octet_stream();
	return mime.essence_str().to_owned();
}

fn get_encoding(headers: HeaderMap) -> EnumSet<Precompression> {
	let mut encoding_set: EnumSet<Precompression> = enum_set!(Precompression::Uncompressed);
	let encoding_option = headers.get(ACCEPT_ENCODING);
	if let Some(encoding) = encoding_option {
		let encoding_string = encoding.to_str().unwrap_or("");

		if encoding_string.contains("gzip") {
			encoding_set.insert(Precompression::Gzip);
		}
		if encoding_string.contains("br") {
			encoding_set.insert(Precompression::Brotli);
		}
	}
	encoding_set
}
