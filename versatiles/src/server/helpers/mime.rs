use std::path::Path;

pub fn guess_mime(path: &Path) -> String {
	let mime = mime_guess::from_path(path).first_or_octet_stream().essence_str().to_owned();
	if mime.starts_with("text/") {
		format!("{mime}; charset=utf-8")
	} else {
		mime
	}
}

#[cfg(test)]
mod tests {
	use super::guess_mime;
	use std::path::Path;

	#[test]
	fn test_guess_mime() {
		let test = |path: &str, mime: &str| {
			assert_eq!(guess_mime(Path::new(path)), mime);
		};

		test("fluffy.css", "text/css; charset=utf-8");
		test("fluffy.gif", "image/gif");
		test("fluffy.htm", "text/html; charset=utf-8");
		test("fluffy.html", "text/html; charset=utf-8");
		test("fluffy.jpeg", "image/jpeg");
		test("fluffy.jpg", "image/jpeg");
		test("fluffy.js", "application/javascript");
		test("fluffy.json", "application/json");
		test("fluffy.pbf", "application/octet-stream");
		test("fluffy.png", "image/png");
		test("fluffy.svg", "image/svg+xml");
	}
}
