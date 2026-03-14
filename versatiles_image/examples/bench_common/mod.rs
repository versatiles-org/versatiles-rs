#![allow(dead_code)]
#![allow(clippy::cast_sign_loss)]

use libwebp_sys::{VP8StatusCode, WebPBitstreamFeatures, WebPDecodeRGB, WebPFree, WebPGetFeatures};
use std::path::PathBuf;

pub const TILE_URLS: &[&str] = &[
	"https://tiles.versatiles.org/tiles/elevation/11/1034/709",
	"https://tiles.versatiles.org/tiles/elevation/11/1044/625",
	"https://tiles.versatiles.org/tiles/elevation/11/1068/728",
	"https://tiles.versatiles.org/tiles/elevation/11/1079/886",
	"https://tiles.versatiles.org/tiles/elevation/11/1081/886",
	"https://tiles.versatiles.org/tiles/elevation/11/1098/660",
	"https://tiles.versatiles.org/tiles/elevation/11/113/896",
	"https://tiles.versatiles.org/tiles/elevation/11/1518/858",
	"https://tiles.versatiles.org/tiles/elevation/11/1569/335",
	"https://tiles.versatiles.org/tiles/elevation/11/693/1105",
	"https://tiles.versatiles.org/tiles/elevation/11/890/437",
];

fn cache_dir() -> PathBuf {
	let dir = PathBuf::from("target/bench_tiles");
	std::fs::create_dir_all(&dir).expect("Failed to create cache directory");
	dir
}

fn cache_path(url: &str) -> PathBuf {
	let name = url.replace("https://", "").replace('/', "_");
	cache_dir().join(format!("{name}.webp"))
}

fn download_tile(url: &str) -> Vec<u8> {
	let path = cache_path(url);
	if path.exists() {
		return std::fs::read(&path).expect("Failed to read cached tile");
	}

	eprintln!("Downloading {url}...");
	let response = reqwest::blocking::get(url).unwrap_or_else(|e| panic!("Failed to download {url}: {e}"));
	assert!(
		response.status().is_success(),
		"HTTP error for {url}: {}",
		response.status()
	);
	let bytes = response
		.bytes()
		.unwrap_or_else(|e| panic!("Failed to read response for {url}: {e}"));
	let data = bytes.to_vec();
	std::fs::write(&path, &data).expect("Failed to cache tile");
	data
}

pub fn load_tile_rgb_data() -> Vec<(String, Vec<u8>, i32, i32)> {
	TILE_URLS
		.iter()
		.map(|url| {
			let data = download_tile(url);
			let label = url
				.rsplit('/')
				.take(3)
				.collect::<Vec<_>>()
				.into_iter()
				.rev()
				.collect::<Vec<_>>()
				.join("/");
			let (pixels, w, h) = decode_webp_to_rgb(&data, url);
			(label, pixels, w, h)
		})
		.collect()
}

fn decode_webp_to_rgb(data: &[u8], label: &str) -> (Vec<u8>, i32, i32) {
	unsafe {
		let mut features: WebPBitstreamFeatures = std::mem::zeroed();
		let status = WebPGetFeatures(data.as_ptr(), data.len(), &raw mut features);
		assert!(
			status == VP8StatusCode::VP8_STATUS_OK,
			"Failed to get features for {label}"
		);
		let mut w: i32 = 0;
		let mut h: i32 = 0;
		let ptr = WebPDecodeRGB(data.as_ptr(), data.len(), &raw mut w, &raw mut h);
		assert!(!ptr.is_null(), "Failed to decode {label}");
		let size = (w as usize) * (h as usize) * 3;
		let pixels = std::slice::from_raw_parts(ptr, size).to_vec();
		WebPFree(ptr.cast());
		(pixels, w, h)
	}
}

pub fn print_header(prefix_columns: &[&str]) {
	let img_cols: Vec<String> = (1..=TILE_URLS.len()).map(|i| format!("img{i}")).collect();
	let all: Vec<&str> = prefix_columns
		.iter()
		.copied()
		.chain(img_cols.iter().map(String::as_str))
		.chain(["total", "time_ms"])
		.collect();
	println!("{}", all.join("\t"));
}

pub fn print_row(prefix_values: &[&str], sizes: &[usize], total_time: std::time::Duration) {
	let total: usize = sizes.iter().sum();
	let time_ms = total_time.as_secs_f64() * 1000.0;
	let total_str = total.to_string();
	let time_str = format!("{time_ms:.1}");
	let size_strs: Vec<String> = sizes.iter().map(std::string::ToString::to_string).collect();
	let all: Vec<&str> = prefix_values
		.iter()
		.copied()
		.chain(size_strs.iter().map(String::as_str))
		.chain([total_str.as_str(), time_str.as_str()])
		.collect();
	println!("{}", all.join("\t"));
}
