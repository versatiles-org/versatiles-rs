//! Minimal in-process HTTP server for tests.
//!
//! Serves files from the workspace `testdata/` directory over HTTP/1.1 with
//! `Range` request support. Tests that exercise the remote (HTTP) tile readers
//! point at this server instead of `download.versatiles.org`, which previously
//! made those tests flaky whenever many CI jobs hit the live server in
//! parallel.

use std::{
	collections::HashMap,
	io::{Read, Write},
	net::{TcpListener, TcpStream},
	path::{Path, PathBuf},
	sync::{Arc, Mutex, OnceLock},
	thread,
};

/// Directory served by the test server (`<workspace>/testdata`).
fn base_dir() -> PathBuf {
	Path::new(env!("CARGO_MANIFEST_DIR")).join("../testdata")
}

/// Lazily-cached file contents, keyed by file name — fixtures are read once.
fn load_file(name: &str) -> Option<Arc<Vec<u8>>> {
	static CACHE: OnceLock<Mutex<HashMap<String, Arc<Vec<u8>>>>> = OnceLock::new();
	let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));

	if let Some(data) = cache.lock().unwrap().get(name) {
		return Some(Arc::clone(data));
	}
	let data = Arc::new(std::fs::read(base_dir().join(name)).ok()?);
	cache.lock().unwrap().insert(name.to_string(), Arc::clone(&data));
	Some(data)
}

/// A localhost HTTP server serving `testdata/` files with byte-range support.
pub struct TestHttpServer {
	port: u16,
}

impl TestHttpServer {
	/// Process-wide shared instance — started once, reused by every test.
	pub fn shared() -> &'static TestHttpServer {
		static SERVER: OnceLock<TestHttpServer> = OnceLock::new();
		SERVER.get_or_init(TestHttpServer::start)
	}

	fn start() -> TestHttpServer {
		let listener = TcpListener::bind("127.0.0.1:0").expect("test HTTP server: bind 127.0.0.1:0");
		let port = listener.local_addr().expect("test HTTP server: local_addr").port();
		thread::spawn(move || {
			for stream in listener.incoming().flatten() {
				thread::spawn(move || handle_connection(stream));
			}
		});
		TestHttpServer { port }
	}

	/// URL for a file under `testdata/`, e.g. `url("berlin.pmtiles")`.
	pub fn url(&self, file: &str) -> String {
		format!("http://127.0.0.1:{}/{file}", self.port)
	}
}

/// Handle one connection: a single `GET`, optionally with a `Range` header.
fn handle_connection(mut stream: TcpStream) {
	// Read the request head, up to the blank line. GET requests have no body.
	let mut buf = Vec::new();
	let mut chunk = [0u8; 2048];
	loop {
		match stream.read(&mut chunk) {
			Ok(0) | Err(_) => return,
			Ok(n) => buf.extend_from_slice(&chunk[..n]),
		}
		if buf.windows(4).any(|w| w == b"\r\n\r\n") {
			break;
		}
		if buf.len() > 32 * 1024 {
			return; // header larger than anything our tests send
		}
	}

	let head = String::from_utf8_lossy(&buf);
	let mut lines = head.lines();
	let mut request = lines.next().unwrap_or_default().split_whitespace();
	let method = request.next().unwrap_or_default();
	let target = request.next().unwrap_or_default();

	// `Range: bytes=START-END`
	let range = lines.take_while(|line| !line.is_empty()).find_map(|line| {
		let (key, value) = line.split_once(':')?;
		key.trim()
			.eq_ignore_ascii_case("range")
			.then(|| value.trim().strip_prefix("bytes="))
			.flatten()
			.map(str::to_string)
	});

	let name = target.split('?').next().unwrap_or_default().trim_start_matches('/');
	if method != "GET" || name.is_empty() || name.contains("..") || name.contains('/') {
		let _ = respond_status(&mut stream, 400, "Bad Request");
		return;
	}
	let Some(data) = load_file(name) else {
		let _ = respond_status(&mut stream, 404, "Not Found");
		return;
	};

	let _ = match range {
		Some(spec) => match parse_range(&spec, data.len()) {
			Some((start, end)) => respond(&mut stream, 206, "Partial Content", &data, Some((start, end))),
			None => respond_status(&mut stream, 416, "Range Not Satisfiable"),
		},
		None => respond(&mut stream, 200, "OK", &data, None),
	};
}

/// Parse a `START-END` range spec; `None` if unsatisfiable for `total` bytes.
///
/// The HTTP reader emits a backwards range (`end == start - 1`) for zero-length
/// reads, which is accepted here and answered with a `206` and an empty body —
/// matching how the production server behaves.
fn parse_range(spec: &str, total: usize) -> Option<(usize, usize)> {
	let (start, end) = spec.split_once('-')?;
	let start: usize = start.trim().parse().ok()?;
	let end: usize = if end.trim().is_empty() {
		total.checked_sub(1)?
	} else {
		end.trim().parse().ok()?
	};
	(start <= total && end < total && end + 1 >= start).then_some((start, end))
}

/// Write a `200` (full body) or `206` (`range` sub-slice) response.
fn respond(
	stream: &mut TcpStream,
	code: u16,
	reason: &str,
	data: &[u8],
	range: Option<(usize, usize)>,
) -> std::io::Result<()> {
	let body: &[u8] = match range {
		Some((start, end)) if start <= end => &data[start..=end],
		Some(_) => &[], // zero-length range (`end == start - 1`)
		None => data,
	};
	let content_range = match range {
		Some((start, end)) => format!("Content-Range: bytes {start}-{end}/{}\r\n", data.len()),
		None => String::new(),
	};
	let header = format!(
		"HTTP/1.1 {code} {reason}\r\n\
		 Content-Type: application/octet-stream\r\n\
		 Accept-Ranges: bytes\r\n\
		 Content-Length: {}\r\n\
		 {content_range}\
		 Connection: close\r\n\r\n",
		body.len(),
	);

	stream.write_all(header.as_bytes())?;
	stream.write_all(body)?;
	stream.flush()
}

/// Write a bodyless error response.
fn respond_status(stream: &mut TcpStream, code: u16, reason: &str) -> std::io::Result<()> {
	let header = format!("HTTP/1.1 {code} {reason}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
	stream.write_all(header.as_bytes())?;
	stream.flush()
}
