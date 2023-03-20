use super::traits::ServerSourceBox;
use crate::helper::{Blob, Precompression};
use axum::{
	async_trait,
	body::{Bytes, Full},
	extract::State,
	extract::{FromRef, FromRequestParts},
	http::{request::Parts, StatusCode},
	middleware::{self, Next},
	response::Response,
	routing::get,
	Router, Server,
};
use enumset::{enum_set, EnumSet};
use http::header::{ACCEPT_ENCODING, CACHE_CONTROL, CONTENT_ENCODING, CONTENT_TYPE};
use std::{
	path::Path,
	sync::{Arc, Mutex},
};
use tokio;

struct TileSource {
	prefix: String,
	source: Arc<ServerSourceBox>,
}

pub struct TileServer {
	ip: String,
	port: u16,
	tile_sources: Vec<TileSource>,
	static_sources: Arc<Mutex<Vec<ServerSourceBox>>>,
}

impl TileServer {
	pub fn new(ip: &str, port: u16) -> TileServer {
		TileServer {
			ip: ip.to_owned(),
			port,
			tile_sources: Vec::new(),
			static_sources: Arc::new(Mutex::new(Vec::new())),
		}
	}

	pub fn add_source(&mut self, url_prefix: String, tile_source: ServerSourceBox) {
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

	pub fn add_static(&mut self, source: ServerSourceBox) {
		log::debug!("set static: source={:?}", source);
		self.static_sources.lock().unwrap().push(source);
	}

	#[tokio::main]
	pub async fn start(&mut self) {
		log::info!("starting server");

		let mut tile_sources_json_lines: Vec<String> = Vec::new();
		/*
			  let mut sources: Vec<(String, usize, Arc<ServerSourceBox>)> = Vec::new();
			  while !self.tile_sources.is_empty() {
				  let (prefix, tile_source) = self.tile_sources.remove(0);
				  let skip = prefix.matches('/').count();
				  tile_sources_json_lines.push(format!(
					  "{{ \"url\":\"{}\", \"name\":\"{}\", \"info\":{} }}",
					  prefix,
					  tile_source.get_name(),
					  tile_source.get_info_as_json()
				  ));
				  sources.push((prefix, skip, Arc::new(tile_source)));
			  }
			  let tile_sources_json: String = "[\n\t".to_owned() + &tile_sources_json_lines.join(",\n\t") + "\n]";

			  let arc_sources = Arc::new(sources);
			  let static_sources: Arc<Mutex<Vec<ServerSourceBox>>> = self.static_sources.clone();
		*/

		// Initiale state
		#[derive(Clone)]
		struct ServerState {}
		let serverState = ServerState {};

		// Initialize App
		let mut app = Router::new().route("/status", get(|| async { "ready!" }));

		for tile_source in self.tile_sources.iter() {
			let skip = tile_source.prefix.matches('/').count();
			tile_sources_json_lines.push(format!(
				"{{ \"url\":\"{}\", \"name\":\"{}\", \"info\":{} }}",
				tile_source.prefix,
				tile_source.source.get_name(),
				tile_source.source.get_info_as_json()
			));

			let route = tile_source.prefix.to_owned() + "*";
			let source = tile_source.source.clone();

			let tileApp = Router::new().route(&route, get(|| async { "tile" })).with_state(source);
			app = app.merge(tileApp);
		}

		//	.with_state(serverState);

		let addr = format!("{}:{}", self.ip, self.port);
		println!("server starts listening on {}", addr);

		Server::bind(&addr.parse().unwrap())
			.serve(app.into_make_service())
			.await
			.expect("server failed");
		/*
		  .serve(

			  function servicemove |req: Request| -> Response {
			  log::debug!("request {:?}", req);

			  let path = urlencoding::decode(req.uri().path()).unwrap().to_string();

			  let _method = req.method();
			  let headers = req.headers();

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

			  if path.starts_with("/api/") {
				  if path.starts_with("/api/status.json") {
					  return ok_data(
						  Blob::from_string("{{\"status\":\"ready\"}}"),
						  &Precompression::Uncompressed,
						  "application/json",
					  );
				  }
				  if path.starts_with("/api/tiles.json") {
					  return ok_data(
						  Blob::from_string(&tile_sources_json),
						  &Precompression::Uncompressed,
						  "application/json",
					  );
				  }
			  }

			  let source_option = arc_sources.iter().find(|(prefix, _, _)| path.starts_with(prefix));

			  let mut sub_path: Vec<&str> = path.split('/').collect();

			  if let Some((_prefix, skip, my_source)) = source_option {
				  // serve tile

				  let source: Arc<ServerSourceBox> = my_source.clone();

				  if skip < &sub_path.len() {
					  sub_path = sub_path.split_off(*skip);
				  } else {
					  sub_path.clear()
				  };

				  log::debug!("try to serve tile {} from {}", sub_path.join("/"), source.get_name());

				  return source.get_data(sub_path.as_slice(), encoding_set);
			  }

			  // serve static content?
			  sub_path.remove(0); // delete first empty element, because of trailing "/"
			  for source in static_sources.lock().unwrap().iter() {
				  log::debug!("try to serve static {} from {}", sub_path.join("/"), source.get_name());

				  let response = source.get_data(sub_path.as_slice(), encoding_set);
				  if response.status() == 200 {
					  return response;
				  }
			  }

			  ok_not_found()
		  })
		  .expect("serve failed");
		*/
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

pub fn guess_mime(path: &Path) -> String {
	let mime = mime_guess::from_path(path).first_or_octet_stream();
	return mime.essence_str().to_owned();
}
