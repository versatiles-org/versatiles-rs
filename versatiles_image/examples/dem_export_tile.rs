#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::many_single_char_names)]

//! Export one terrarium DEM tile in three versions — original, uniform (current
//! `dem_quantize`), and TV-within-tube — as WebP files for visual inspection.
//!
//! Uses the dem_quantize defaults: elevation_error=0.1, slope_error=1.0°, WebP method 6.
//!
//! Usage: cargo run --release --example dem_export_tile

mod bench_common;

use bench_common::{load_tile_rgb_data, original_blobs};
use libwebp_sys::{
	WebPConfig, WebPEncode, WebPFree, WebPMemoryWrite, WebPMemoryWriter, WebPMemoryWriterInit, WebPPicture,
	WebPPictureFree, WebPPictureImportRGB,
};
use std::path::PathBuf;

const TARGET: &str = "11/1068/728.webp";
const Z: u32 = 11;
const Y: u32 = 728;
const ELEVATION_ERROR: f64 = 0.1;
const SLOPE_ERROR_DEG: f64 = 1.0;
const RAW_UNIT_M: f64 = 1.0 / 256.0;
const WORLD_SIZE: f64 = 40_075_016.686;

fn pixel_meters(z: u32, y: u32) -> f64 {
	let n = f64::from(1u32 << z);
	let lat = (std::f64::consts::PI * (1.0 - 2.0 * (f64::from(y) + 0.5) / n))
		.sinh()
		.atan();
	(WORLD_SIZE / n) * lat.cos() / 256.0
}

// Matches production dem_quantize: floor(log2(tol_raw + 1)).
fn zero_bits_for(tol_raw: f64) -> u32 {
	if tol_raw < 1.0 {
		0
	} else {
		((tol_raw + 1.0).log2().floor() as i32).clamp(0, 24) as u32
	}
}

fn quantize_raw_bits(raw: i64, bits: u32) -> i64 {
	if bits == 0 {
		return raw;
	}
	let step = 1i64 << bits;
	(raw + step / 2).clamp(0, 0x00FF_FFFF) & !(step - 1)
}

/// Rounded uniform quantization (current dem_quantize).
fn uniform(e_raw: &[i64], pm: f64) -> Vec<i64> {
	let elev_tol = ELEVATION_ERROR * pm / RAW_UNIT_M;
	let slope_tol = pm * SLOPE_ERROR_DEG.to_radians().tan() / RAW_UNIT_M;
	let zb = zero_bits_for(elev_tol.min(slope_tol));
	e_raw.iter().map(|&r| quantize_raw_bits(r, zb)).collect()
}

/// TV-within-tube (feasible-interval flattening; see dem_rd_bench for details).
/// `se` is the slope budget in degrees (may be tightened to leave room for grid snapping).
fn tv(e_raw: &[i64], w: usize, h: usize, pm: f64, se: f64) -> Vec<i64> {
	let elev_tol = ELEVATION_ERROR * pm / RAW_UNIT_M;
	let slope_tol = pm * se.to_radians().tan() / RAW_UNIT_M;
	let zb = zero_bits_for(elev_tol.min(slope_tol));
	let step: i64 = if zb == 0 { 1 } else { 1i64 << zb };
	let step_f = step as f64;
	let mut ep = vec![0i64; e_raw.len()];
	for y in 0..h {
		for x in 0..w {
			let p = y * w + x;
			let er = e_raw[p] as f64;
			let mut lo = er - elev_tol;
			let mut hi = er + elev_tol;
			if x > 0 {
				let c = ep[p - 1] as f64 + (e_raw[p] - e_raw[p - 1]) as f64;
				lo = lo.max(c - slope_tol);
				hi = hi.min(c + slope_tol);
			}
			if y > 0 {
				let c = ep[p - w] as f64 + (e_raw[p] - e_raw[p - w]) as f64;
				lo = lo.max(c - slope_tol);
				hi = hi.min(c + slope_tol);
			}
			let val = if x > 0 && (ep[p - 1] as f64) >= lo && (ep[p - 1] as f64) <= hi {
				ep[p - 1]
			} else if y > 0 && (ep[p - w] as f64) >= lo && (ep[p - w] as f64) <= hi {
				ep[p - w]
			} else if lo <= hi {
				let t = er.clamp(lo, hi);
				let mut c = (t / step_f).round() as i64 * step;
				if (c as f64) < lo {
					c += step;
				}
				if (c as f64) > hi {
					c -= step;
				}
				if (c as f64) >= lo && (c as f64) <= hi {
					c
				} else {
					t.round() as i64
				}
			} else {
				f64::midpoint(lo, hi).round() as i64
			};
			ep[p] = val.clamp(0, 0x00FF_FFFF);
		}
	}
	ep
}

/// Combined: TV flattening at a tightened slope budget `se_tv`, then snapped onto the
/// uniform step grid. The margin (SLOPE − se_tv) leaves room for the snap so the final
/// result stays within the real slope budget while keeping clean low bits.
fn combined(e_raw: &[i64], w: usize, h: usize, pm: f64, se_tv: f64) -> Vec<i64> {
	let t = tv(e_raw, w, h, pm, se_tv);
	let elev_tol = ELEVATION_ERROR * pm / RAW_UNIT_M;
	let slope_tol = pm * SLOPE_ERROR_DEG.to_radians().tan() / RAW_UNIT_M;
	let zb = zero_bits_for(elev_tol.min(slope_tol));
	if zb == 0 {
		return t;
	}
	let step = 1i64 << zb;
	let step_f = step as f64;
	t.iter()
		.map(|&r| ((r as f64 / step_f).round() as i64 * step).clamp(0, 0x00FF_FFFF))
		.collect()
}

/// Encode a raw terrarium grid as lossless WebP (method 6, like production).
fn encode_webp(raw: &[i64], w: i32, h: i32) -> Vec<u8> {
	let mut rgb = Vec::with_capacity((w * h * 3) as usize);
	for &r in raw {
		rgb.push(((r >> 16) & 0xFF) as u8);
		rgb.push(((r >> 8) & 0xFF) as u8);
		rgb.push((r & 0xFF) as u8);
	}
	unsafe {
		let mut config = WebPConfig::new().unwrap();
		config.lossless = 1;
		config.method = 6;
		config.quality = 100.0;
		config.exact = 1;
		let mut picture = WebPPicture::new().unwrap();
		picture.use_argb = 1;
		picture.width = w;
		picture.height = h;
		assert!(
			WebPPictureImportRGB(&raw mut picture, rgb.as_ptr(), w * 3) != 0,
			"import failed"
		);
		let mut writer: WebPMemoryWriter = std::mem::zeroed();
		WebPMemoryWriterInit(&raw mut writer);
		picture.writer = Some(WebPMemoryWrite);
		picture.custom_ptr = (&raw mut writer).cast();
		let ok = WebPEncode(&raw const config, &raw mut picture);
		WebPPictureFree(&raw mut picture);
		assert!(ok != 0, "encode failed");
		let out = std::slice::from_raw_parts(writer.mem, writer.size).to_vec();
		WebPFree(writer.mem.cast());
		out
	}
}

#[allow(clippy::too_many_lines)]
fn main() {
	let images = load_tile_rgb_data();
	let blobs = original_blobs();
	let (label, pixels, w, h) = images
		.iter()
		.find(|(l, ..)| l == TARGET)
		.expect("target tile not found");
	let orig_blob = &blobs
		.iter()
		.find(|(l, _)| l == TARGET)
		.expect("target blob not found")
		.1;
	let (wu, hu) = (*w as usize, *h as usize);
	let pm = pixel_meters(Z, Y);

	let e_raw: Vec<i64> = (0..wu * hu)
		.map(|i| (i64::from(pixels[i * 3]) << 16) | (i64::from(pixels[i * 3 + 1]) << 8) | i64::from(pixels[i * 3 + 2]))
		.collect();

	let uni_grid = uniform(&e_raw, pm);
	let tv_grid = tv(&e_raw, wu, hu, pm, SLOPE_ERROR_DEG);

	// Diagnose blue-channel noise: how many pixels are off the uniform step grid?
	let slope_tol = pm * SLOPE_ERROR_DEG.to_radians().tan() / RAW_UNIT_M;
	let elev_tol = ELEVATION_ERROR * pm / RAW_UNIT_M;
	let step: i64 = 1i64 << zero_bits_for(elev_tol.min(slope_tol));
	let stats = |g: &[i64], name: &str| {
		let off = g.iter().filter(|&&r| r % step != 0).count();
		let mut bs: Vec<i64> = g.iter().map(|&r| r & 0xFF).collect();
		bs.sort_unstable();
		bs.dedup();
		println!(
			"  {name}: step={step}, off-grid={off}/{}, distinct B={}",
			g.len(),
			bs.len()
		);
	};
	// Realised slope error (degrees) check vs the budget, for validity.
	let max_slope = |g: &[i64]| {
		let mut m = 0.0f64;
		for y in 0..hu {
			for x in 0..wu {
				let i = y * wu + x;
				let qi = (g[i] - e_raw[i]) as f64;
				if x + 1 < wu {
					m = m.max(
						((qi - (g[i + 1] - e_raw[i + 1]) as f64).abs() * RAW_UNIT_M / pm)
							.atan()
							.to_degrees(),
					);
				}
				if y + 1 < hu {
					m = m.max(
						((qi - (g[i + wu] - e_raw[i + wu]) as f64).abs() * RAW_UNIT_M / pm)
							.atan()
							.to_degrees(),
					);
				}
			}
		}
		m
	};
	// Sweep the TV slope margin: run TV at a tightened budget, then snap to grid; keep the
	// largest tightened budget whose snapped result still respects the real 1.0° budget.
	println!("  combined margin sweep (TV budget → combined slope, size):");
	let mut comb_grid = combined(&e_raw, wu, hu, pm, SLOPE_ERROR_DEG);
	let mut chosen_se = SLOPE_ERROR_DEG;
	for &se_tv in &[1.0f64, 0.9, 0.8, 0.7, 0.6, 0.5] {
		let g = combined(&e_raw, wu, hu, pm, se_tv);
		let ms = max_slope(&g);
		let sz = encode_webp(&g, *w, *h).len();
		let ok = ms <= SLOPE_ERROR_DEG + 1e-3;
		println!(
			"    TV@{se_tv:.1}° → slope={ms:.3}° size={sz} {}",
			if ok { "VALID" } else { "over" }
		);
		if ok {
			comb_grid = g;
			chosen_se = se_tv;
			break;
		}
	}
	println!("  chosen combined: TV@{chosen_se:.1}°");
	stats(&uni_grid, "uniform");
	stats(&tv_grid, "tv");
	stats(&comb_grid, "combined");
	println!(
		"  max slope error (budget {SLOPE_ERROR_DEG}°): uniform={:.3}° tv={:.3}° combined={:.3}°",
		max_slope(&uni_grid),
		max_slope(&tv_grid),
		max_slope(&comb_grid)
	);

	let uniform_blob = encode_webp(&uni_grid, *w, *h);
	let tv_blob = encode_webp(&tv_grid, *w, *h);
	let comb_blob = encode_webp(&comb_grid, *w, *h);

	let dir = PathBuf::from("target/tile_export");
	std::fs::create_dir_all(&dir).expect("create dir");
	let stem = label.replace(['/', '.'], "_");
	let files = [
		(format!("{stem}_original.webp"), orig_blob.clone()),
		(format!("{stem}_uniform.webp"), uniform_blob),
		(format!("{stem}_tv.webp"), tv_blob),
		(format!("{stem}_combined.webp"), comb_blob),
	];
	println!("tile {label}  ({w}x{h}, pixel≈{pm:.1} m)\n");
	for (name, data) in &files {
		let path = dir.join(name);
		std::fs::write(&path, data).expect("write file");
		println!(
			"{:>10} B  {}",
			data.len(),
			path.canonicalize().unwrap_or(path).display()
		);
	}
}
