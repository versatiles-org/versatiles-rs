use super::types::ServerSourceBox;
use hyper::{
	service::{make_service_fn, service_fn},
	Body, Client, Method, Request, Response, Result, Server, StatusCode,
};
use std::{collections::HashMap, net::SocketAddr};
use tokio::fs::File;

type GenericError = Box<dyn std::error::Error + Send + Sync>;

static NOTFOUND: &[u8] = b"Not Found";

pub struct TileServer {
	port: u16,
	sources: HashMap<String, ServerSourceBox>,
}

impl TileServer {
	pub fn new(port: u16) -> TileServer {
		return TileServer {
			port,
			sources: HashMap::new(),
		};
	}

	pub fn add_source(&mut self, name: &str, source: ServerSourceBox) {
		if self.sources.contains_key(name) {
			panic!("multiple sources with the url '{}' are defined", name)
		};
		self.sources.insert(name.to_owned(), source);
	}

	#[tokio::main]
	pub async fn start(&self) {
		let client = Client::new();
		let addr = SocketAddr::from(([127, 0, 0, 1], self.port));
		let new_service = make_service_fn(move |_| {
			// Move a clone of `client` into the `service_fn`.
			let _client = client.clone();
			async { Ok::<_, GenericError>(service_fn(response_examples)) }
		});
		let server = Server::bind(&addr).serve(new_service);

		if let Err(e) = server.await {
			eprintln!("server error: {}", e);
		}

		async fn response_examples(req: Request<Body>) -> Result<Response<Body>> {
			match (req.method(), req.uri().path()) {
				(&Method::GET, "/") | (&Method::GET, "/index.html") => make_text_content("index"),
				(&Method::GET, "/no_file.html") => {
					// Test what happens when file cannot be be found
					simple_file_send("this_file_should_not_exist.html").await
				}
				_ => Ok(not_found()),
			}
		}

		/// HTTP status code 404
		fn not_found() -> Response<Body> {
			Response::builder()
				.status(StatusCode::NOT_FOUND)
				.body(NOTFOUND.into())
				.unwrap()
		}

		async fn simple_file_send(filename: &str) -> Result<Response<Body>> {
			// Serve a file by asynchronously reading it by chunks using tokio-util crate.

			if let Ok(_file) = File::open(filename).await {
				let body = Body::from("stream");
				return Ok(Response::new(body));
			}

			Ok(not_found())
		}

		fn make_text_content(content: &str) -> Result<Response<Body>> {
			return Ok(Response::new(Body::from(content.to_string())));
		}
	}

	pub fn iter_url_mapping(&self) -> impl Iterator<Item = (&str, &str)> {
		self
			.sources
			.iter()
			.map(|(url, source)| (url.as_str(), source.get_name()))
	}
}
