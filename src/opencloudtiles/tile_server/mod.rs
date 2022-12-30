use super::containers::abstract_container::TileReader;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Client, Method, Request, Response, Result, Server, StatusCode};
use std::collections::HashMap;
use std::net::SocketAddr;
use tokio::fs::File;

type GenericError = Box<dyn std::error::Error + Send + Sync>;

static INDEX: &str = "examples/send_file_index.html";
static NOTFOUND: &[u8] = b"Not Found";

pub struct TileServer {
	port: u16,
	sources: HashMap<String, Box<dyn TileReader>>,
}

impl TileServer {
	pub fn new(port: u16) -> TileServer {
		return TileServer {
			port,
			sources: HashMap::new(),
		};
	}
	pub fn add_source(&mut self, name: &str, reader: Box<dyn TileReader>) {
		self.sources.insert(name.to_owned(), reader);
	}
	#[tokio::main]
	pub async fn start(&self) {
		let client = Client::new();
		let addr = SocketAddr::from(([127, 0, 0, 1], self.port));
		let new_service = make_service_fn(move |_| {
			// Move a clone of `client` into the `service_fn`.
			let client = client.clone();
			async { Ok::<_, GenericError>(service_fn(response_examples)) }
		});
		let server = Server::bind(&addr).serve(new_service);

		if let Err(e) = server.await {
			eprintln!("server error: {}", e);
		}

		async fn response_examples(req: Request<Body>) -> Result<Response<Body>> {
			match (req.method(), req.uri().path()) {
				(&Method::GET, "/") | (&Method::GET, "/index.html") => simple_file_send(INDEX).await,
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

			if let Ok(file) = File::open(filename).await {
				let body = Body::from("stream");
				return Ok(Response::new(body));
			}

			Ok(not_found())
		}
	}
	/*
	async fn serve(&self) {

		async |req: Request<Body>| -> Result<Response<Body>, Infallible> {
			let uri = req.uri();
			let path = uri.into_parts().path_and_query.unwrap().path();
			let parts = path.split('/').collect();
			match parts[1] {
				Some("static") => return Ok(self.serve_static(&parts[2..], req).await),
			}
		},
	}
	async fn serve_static(&self, url_parts: &Vec<&str>, req: Request<Body>) -> Response<Body> {
		return Response::new(Body::from("Hello World"));
	}*/
}
