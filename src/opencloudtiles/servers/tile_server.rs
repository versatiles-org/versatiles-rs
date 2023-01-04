use super::types::ServerSourceBox;
use crate::opencloudtiles::types::Compression;
use enumset::{enum_set, EnumSet};
use hyper::{
	header,
	service::{make_service_fn, service_fn},
	Body, Request, Response, Result, Server, StatusCode,
};
use std::{
	net::SocketAddr,
	sync::{Arc, Mutex},
};
use tokio::fs::File;

type GenericError = Box<dyn std::error::Error + Send + Sync>;

static NOTFOUND: &[u8] = b"Not Found";

pub struct TileServer {
	port: u16,
	sources: Vec<(String, ServerSourceBox)>,
}

impl TileServer {
	pub fn new(port: u16) -> TileServer {
		return TileServer {
			port,
			sources: Vec::new(),
		};
	}

	pub fn add_source(&mut self, url_prefix: String, source: ServerSourceBox) {
		let mut prefix = url_prefix;
		if !prefix.starts_with("/") {
			prefix = "/".to_owned() + &prefix;
		}
		if !prefix.ends_with("/") {
			prefix = prefix + "/";
		}

		for (other_prefix, _source) in self.sources.iter() {
			if other_prefix.starts_with(&prefix) || prefix.starts_with(other_prefix) {
				panic!(
					"multiple sources with the prefix '{}' and '{}' are defined",
					prefix, other_prefix
				);
			};
		}

		self.sources.push((prefix, source));
	}

	#[tokio::main]
	pub async fn start(&self) {
		fn ok_not_found() -> Result<Response<Body>> {
			Ok(Response::builder()
				.status(StatusCode::NOT_FOUND)
				.body(NOTFOUND.into())
				.unwrap())
		}

		let addr = SocketAddr::from(([127, 0, 0, 1], self.port));

		let mutex_sources1 = Arc::new(Mutex::new(&self.sources));

		let new_service = make_service_fn(move |_| {
			let mutex_sources1 = mutex_sources1.clone();
			async move {
				Ok::<_, GenericError>(service_fn(move |req: Request<Body>| {
					let mutex_sources1 = mutex_sources1.clone();
					async move {
						let method = req.method();
						let path = req.uri().path();
						let headers = req.headers();

						let mut encoding_set: EnumSet<Compression> = enum_set!(Compression::Uncompressed);
						let encoding = headers.get(header::ACCEPT_ENCODING);
						if encoding.is_some() {
							let encoding_string = encoding.unwrap().to_str().unwrap_or("");

							if encoding_string.contains("gzip") {
								encoding_set.insert(Compression::Gzip);
							}
							if encoding_string.contains("br") {
								encoding_set.insert(Compression::Brotli);
							}
						}

						for (prefix, source) in mutex_sources1.lock().unwrap().iter() {}

						return ok_not_found();
					}
				}))
			}
		});
		let server = Server::bind(&addr).serve(new_service);

		if let Err(e) = server.await {
			eprintln!("server error: {}", e);
		}

		/// HTTP status code 404

		async fn simple_file_send(filename: &str) -> Result<Response<Body>> {
			// Serve a file by asynchronously reading it by chunks using tokio-util crate.

			if let Ok(_file) = File::open(filename).await {
				let body = Body::from("stream");
				return Ok(Response::new(body));
			}

			return ok_not_found();
		}

		fn make_text_content(content: &str) -> Result<Response<Body>> {
			return Ok(Response::new(Body::from(content.to_string())));
		}
	}

	pub fn iter_url_mapping(&self) -> impl Iterator<Item = (String, String)> + '_ {
		self
			.sources
			.iter()
			.map(|(url, source)| (url.to_owned(), source.get_name().to_owned()))
	}
}
