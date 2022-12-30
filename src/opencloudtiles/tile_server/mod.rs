use super::containers::abstract_container::TileReader;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, Server};
use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;

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
		async fn handle_request(_req: Request<Body>) -> Result<Response<Body>, Infallible> {
			Ok(Response::new("Hello, World".into()))
		}

		let addr = SocketAddr::from(([127, 0, 0, 1], self.port));
		let make_svc = make_service_fn(|_conn| async { Ok::<_, Infallible>(service_fn(handle_request)) });
		let server = Server::bind(&addr).serve(make_svc);

		if let Err(e) = server.await {
			eprintln!("server error: {}", e);
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
