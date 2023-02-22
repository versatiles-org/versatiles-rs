use super::traits::ServerSourceBox;
use crate::helper::{Blob, Precompression};
use enumset::{enum_set, EnumSet};
use hyper::{
	header::{self, CONTENT_ENCODING, CONTENT_TYPE},
	service::{make_service_fn, service_fn},
	Body, Request, Response, Result, Server, StatusCode,
};
use std::{
	net::{IpAddr, SocketAddr},
	path::Path,
	sync::Arc,
};

type GenericError = Box<dyn std::error::Error + Send + Sync>;

pub struct TileServer {
	ip: IpAddr,
	port: u16,
	sources: Vec<(String, ServerSourceBox)>,
	static_source: Option<Arc<ServerSourceBox>>,
}

impl TileServer {
	pub fn new(ip: IpAddr, port: u16) -> TileServer {
		TileServer {
			ip,
			port,
			sources: Vec::new(),
			static_source: None,
		}
	}

	pub fn add_source(&mut self, url_prefix: String, source: ServerSourceBox) {
		log::info!("add source: prefix='{}', source={:?}", url_prefix, source);

		let mut prefix = url_prefix;
		if !prefix.starts_with('/') {
			prefix = "/".to_owned() + &prefix;
		}
		if !prefix.ends_with('/') {
			prefix += "/";
		}

		for (other_prefix, _source) in self.sources.iter() {
			if other_prefix.starts_with(&prefix) || prefix.starts_with(other_prefix) {
				panic!("multiple sources with the prefix '{prefix}' and '{other_prefix}' are defined");
			};
		}

		self.sources.push((prefix, source));
	}

	pub fn set_static(&mut self, source: ServerSourceBox) {
		log::info!("set static: source={:?}", source);
		self.static_source = Some(Arc::new(source));
	}

	#[tokio::main]
	pub async fn start(&mut self) {
		log::info!("starting server");

		let socket = SocketAddr::new(self.ip, self.port);

		let mut sources: Vec<(String, usize, Arc<ServerSourceBox>)> = Vec::new();
		while !self.sources.is_empty() {
			let (prefix, source) = self.sources.pop().unwrap();
			let skip = prefix.matches('/').count();
			sources.push((prefix, skip, Arc::new(source)));
		}
		let arc_sources = Arc::new(sources);
		let arc_static_source = self.static_source.clone();

		let new_service = make_service_fn(move |_| {
			log::debug!("new service");

			let arc_sources = arc_sources.clone();
			let arc_static_source = arc_static_source.clone();
			async move {
				Ok::<_, GenericError>(service_fn(move |req: Request<Body>| {
					let arc_sources = arc_sources.clone();
					let arc_static_source = arc_static_source.clone();

					async move {
						log::debug!("request {:?}", req);

						let path = urlencoding::decode(req.uri().path()).unwrap().to_string();

						let _method = req.method();
						let headers = req.headers();

						let mut encoding_set: EnumSet<Precompression> = enum_set!(Precompression::Uncompressed);
						let encoding_option = headers.get(header::ACCEPT_ENCODING);
						if let Some(encoding) = encoding_option {
							let encoding_string = encoding.to_str().unwrap_or("");

							if encoding_string.contains("gzip") {
								encoding_set.insert(Precompression::Gzip);
							}
							if encoding_string.contains("br") {
								encoding_set.insert(Precompression::Brotli);
							}
						}

						let source_option = arc_sources.iter().find(|(prefix, _, _)| path.starts_with(prefix));

						let mut sub_path: Vec<&str> = path.split('/').collect();
						let source: Arc<ServerSourceBox>;
						if let Some((_prefix, skip, my_source)) = source_option {
							source = my_source.clone();

							if skip < &sub_path.len() {
								sub_path = sub_path.split_off(*skip);
							} else {
								sub_path.clear()
							};
						} else if arc_static_source.is_some() {
							source = arc_static_source.as_ref().unwrap().clone();
						} else {
							return ok_not_found();
						}

						let result = source.get_data(sub_path.as_slice(), encoding_set);

						if result.is_err() {
							return ok_not_found();
						}

						result
					}
				}))
			}
		});
		let server = Server::bind(&socket).serve(new_service);
		println!("server is running");

		if let Err(e) = server.await {
			eprintln!("server error: {e}");
		}
	}

	pub fn iter_url_mapping(&self) -> impl Iterator<Item = (String, String)> + '_ {
		self
			.sources
			.iter()
			.map(|(url, source)| (url.to_owned(), source.get_name().to_owned()))
	}
}

pub fn ok_not_found() -> Result<Response<Body>> {
	Ok(Response::builder()
		.status(StatusCode::NOT_FOUND)
		.body("Not Found".into())
		.unwrap())
}

pub fn ok_data(data: Blob, precompression: &Precompression, mime: &str) -> Result<Response<Body>> {
	let mut response = Response::builder().status(StatusCode::OK).header(CONTENT_TYPE, mime);

	match precompression {
		Precompression::Uncompressed => {}
		Precompression::Gzip => response = response.header(CONTENT_ENCODING, "gzip"),
		Precompression::Brotli => response = response.header(CONTENT_ENCODING, "br"),
	}

	Ok(response.body(data.as_vec().into()).unwrap())
}

pub fn guess_mime(path: &Path) -> String {
	let mime = mime_guess::from_path(path).first_or_octet_stream();
	return mime.essence_str().to_owned();
}
