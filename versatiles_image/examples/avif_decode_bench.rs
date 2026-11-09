//! Benchmark for AVIF decoding with rav1d.
//!
//! Usage:
//!   cargo run --release -p versatiles_image --example avif_decode_bench -- bench <dir>
//!   cargo run --release -p versatiles_image --example avif_decode_bench -- topng <in.avif> <out.png>
//!
//! `bench` takes every `.webp` and `.png` in `<dir>`, encodes it as AVIF at two qualities and
//! times decoding the AVIF against decoding the original. `topng` decodes one AVIF file, to
//! compare with a reference decoder.

use std::{path::Path, time::Instant};

use versatiles_core::{Blob, TileFormat};
use versatiles_image::{DynamicImage, decode, encode};

const RUNS: usize = 20;

/// Median time of `RUNS` calls, in milliseconds.
fn time_ms(mut f: impl FnMut()) -> f64 {
	let mut times: Vec<f64> = (0..RUNS)
		.map(|_| {
			let start = Instant::now();
			f();
			start.elapsed().as_secs_f64() * 1000.0
		})
		.collect();
	times.sort_by(f64::total_cmp);
	times[RUNS / 2]
}

/// Peak signal-to-noise ratio of `b` against `a`, both converted to RGB.
fn psnr(a: &DynamicImage, b: &DynamicImage) -> f64 {
	let (a, b) = (a.to_rgb8(), b.to_rgb8());
	let mse = a
		.as_raw()
		.iter()
		.zip(b.as_raw())
		.map(|(x, y)| (f64::from(*x) - f64::from(*y)).powi(2))
		.sum::<f64>()
		/ a.as_raw().len() as f64;
	10.0 * (255.0 * 255.0 / mse).log10()
}

fn bench(dir: &Path) {
	let mut files: Vec<_> = std::fs::read_dir(dir)
		.expect("read dir")
		.map(|e| e.expect("dir entry").path())
		.filter(|p| matches!(p.extension().and_then(|e| e.to_str()), Some("webp" | "png")))
		.collect();
	files.sort();

	println!(
		"{:<28} {:>9} {:>8} {:>3} {:>8} {:>9} {:>9} {:>7}",
		"tile", "orig dec", "orig B", "q", "avif B", "avif enc", "avif dec", "PSNR"
	);
	for path in files {
		let name = path.file_name().unwrap().to_string_lossy().to_string();
		let format = if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("png")) {
			TileFormat::PNG
		} else {
			TileFormat::WEBP
		};
		let original = Blob::from(std::fs::read(&path).expect("read tile"));
		let image = decode(&original, format).expect("decode original");
		let orig_ms = time_ms(|| {
			decode(&original, format).unwrap();
		});

		for quality in [92, 50] {
			let start = Instant::now();
			let avif = encode(&image, TileFormat::AVIF, Some(quality), None).expect("encode AVIF");
			let enc_ms = start.elapsed().as_secs_f64() * 1000.0;
			let decoded = decode(&avif, TileFormat::AVIF).expect("decode AVIF");
			let dec_ms = time_ms(|| {
				decode(&avif, TileFormat::AVIF).unwrap();
			});
			println!(
				"{name:<28} {orig_ms:>7.2}ms {:>8} {quality:>3} {:>8} {enc_ms:>7.1}ms {dec_ms:>7.2}ms {:>5.1}dB",
				original.len(),
				avif.len(),
				psnr(&image, &decoded)
			);
		}
	}
}

fn topng(input: &Path, output: &Path) {
	let blob = Blob::from(std::fs::read(input).expect("read AVIF"));
	let image = decode(&blob, TileFormat::AVIF).expect("decode AVIF");
	println!(
		"{}: {}x{} {:?}",
		input.display(),
		image.width(),
		image.height(),
		image.color()
	);
	image.save(output).expect("write PNG");
}

fn main() {
	let args: Vec<String> = std::env::args().collect();
	match args.get(1).map(String::as_str) {
		Some("bench") => bench(Path::new(&args[2])),
		Some("topng") => topng(Path::new(&args[2]), Path::new(&args[3])),
		_ => eprintln!("usage: avif_decode_bench bench <dir> | topng <in.avif> <out.png>"),
	}
}
