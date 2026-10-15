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
	sync::{
		Arc, Mutex, OnceLock,
		atomic::{AtomicUsize, Ordering},
	},
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
	/// Requests that have arrived, per slow file name; see [`Self::requests_to`].
	slow_requests: SlowRequests,
}

/// Requests that have arrived, per slow file name.
type SlowRequests = Arc<Mutex<HashMap<String, usize>>>;

impl TestHttpServer {
	/// Process-wide shared instance — started once, reused by every test.
	#[must_use]
	pub fn shared() -> &'static TestHttpServer {
		static SERVER: OnceLock<TestHttpServer> = OnceLock::new();
		SERVER.get_or_init(TestHttpServer::start)
	}

	fn start() -> TestHttpServer {
		let listener = TcpListener::bind("127.0.0.1:0").expect("test HTTP server: bind 127.0.0.1:0");
		let port = listener.local_addr().expect("test HTTP server: local_addr").port();
		let slow_requests = SlowRequests::default();
		let counter = Arc::clone(&slow_requests);
		thread::spawn(move || {
			for stream in listener.incoming().flatten() {
				let counter = Arc::clone(&counter);
				thread::spawn(move || handle_connection(stream, &counter));
			}
		});
		TestHttpServer { port, slow_requests }
	}

	/// URL for a file under `testdata/`, e.g. `url("berlin.pmtiles")`.
	#[must_use]
	pub fn url(&self, file: &str) -> String {
		format!("http://127.0.0.1:{}/{file}", self.port)
	}

	/// URL for the same file, with every request to it answered only after
	/// `delay_ms`: a remote source that is slow, for tests that need a request
	/// to still be running when something else happens.
	///
	/// Every call returns a URL of its own, so that [`Self::requests_to`] counts
	/// only the requests of the test that asked for it, although tests running
	/// in parallel share this server.
	#[must_use]
	#[allow(
		dead_code,
		reason = "this file is compiled into two test targets, and only the end-to-end one uses this; \
		          `expect` would be unfulfilled there"
	)]
	pub fn slow_url(&self, file: &str, delay_ms: u64) -> String {
		static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
		let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
		format!("http://127.0.0.1:{}/{SLOW_PREFIX}{delay_ms}-{id}-{file}", self.port)
	}

	/// How many requests to a [`Self::slow_url`] have arrived, counted when they
	/// arrive rather than when they are answered. That is what tells a test the
	/// request it caused has reached the source, which a sleep only hopes.
	#[must_use]
	#[allow(
		dead_code,
		reason = "this file is compiled into two test targets, and only the end-to-end one uses this; \
		          `expect` would be unfulfilled there"
	)]
	pub fn requests_to(&self, url: &str) -> usize {
		let name = url.rsplit('/').next().unwrap_or_default();
		self.slow_requests.lock().unwrap().get(name).copied().unwrap_or(0)
	}
}

/// What a file name starts with to have its requests delayed:
/// `delay-<milliseconds>-<id>-<file>`. A prefix, not a query, so that the name
/// still ends in the file's extension for whoever picks a reader by it.
const SLOW_PREFIX: &str = "delay-";

/// Handle one connection: a single `GET`, optionally with a `Range` header.
/// A request to a slow file is counted in `slow_requests` when it arrives.
fn handle_connection(mut stream: TcpStream, slow_requests: &SlowRequests) {
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
	let slow = name
		.strip_prefix(SLOW_PREFIX)
		.and_then(|rest| rest.split_once('-'))
		.and_then(|(delay_ms, rest)| Some((delay_ms, rest.split_once('-')?.1)));
	let name = match slow {
		Some((delay_ms, file)) => {
			*slow_requests.lock().unwrap().entry(name.to_string()).or_default() += 1;
			thread::sleep(std::time::Duration::from_millis(delay_ms.parse().unwrap_or(0)));
			file
		}
		None => name,
	};
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
