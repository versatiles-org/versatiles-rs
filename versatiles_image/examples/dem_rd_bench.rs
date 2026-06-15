#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::cast_lossless)]
#![allow(clippy::many_single_char_names)]
#![allow(dead_code)]

//! Rate–distortion benchmark for terrarium DEM size-reduction strategies.
//!
//! For each real terrarium elevation tile, we decode to elevation, derive a per-tile
//! error tolerance from the same two physical criteria `dem_quantize` uses
//! (elevation error vs pixel size, and max slope distortion — stricter wins), then
//! apply several candidate transforms, re-encode as terrarium WebP (lossless,
//! method=4) and report BOTH the resulting byte size and the realised errors:
//!   - elevation difference (max / RMS), in metres
//!   - slope error (max / RMS), in degrees
//!
//! Candidates:
//!   orig         decode → re-encode unchanged (size reference, ~0 error)
//!   mask         current dem_quantize: truncating bit-mask (AND)
//!   rmask        rounded bit-mask (add half-step before AND) — unbiased
//!   rmask+1      rounded bit-mask, one extra bit (rounding halves error → afford it)
//!   round        round to nearest arbitrary step Q (non-power-of-two)
//!   smooth       clamped iterative smoothing inside the ±tol tube
//!   smooth+round smoothing then round to step Q
//!
//! Usage: cargo run --release --example dem_rd_bench

mod bench_common;

use bench_common::load_tile_rgb_data;
use libwebp_sys::{
	WebPConfig, WebPEncode, WebPFree, WebPMemoryWrite, WebPMemoryWriter, WebPMemoryWriterClear, WebPMemoryWriterInit,
	WebPPicture, WebPPictureFree, WebPPictureImportRGB,
};

const WORLD_SIZE: f64 = 40_075_016.686;
const ELEVATION_ERROR: f64 = 0.5; // fraction of pixel ground size
const SLOPE_ERROR_DEG: f64 = 1.0;
const RAW_UNIT_M: f64 = 1.0 / 256.0; // terrarium: 1 raw unit = 1/256 m
const SMOOTH_ITERS: usize = 12;

// ── terrarium codec ──────────────────────────────────────────────────────────
fn rgb_to_elev(r: u8, g: u8, b: u8) -> f64 {
	let raw = (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b);
	raw as f64 * RAW_UNIT_M - 32768.0
}
fn elev_to_rgb(e: f64) -> [u8; 3] {
	let raw = ((e + 32768.0) / RAW_UNIT_M).round().clamp(0.0, 0x00FF_FFFF as f64) as u32;
	[
		((raw >> 16) & 0xFF) as u8,
		((raw >> 8) & 0xFF) as u8,
		(raw & 0xFF) as u8,
	]
}

// ── geo helpers ──────────────────────────────────────────────────────────────
fn pixel_meters(z: u32, _x: u32, y: u32) -> f64 {
	let n = f64::from(1u32 << z);
	let lat_rad = (std::f64::consts::PI * (1.0 - 2.0 * (f64::from(y) + 0.5) / n))
		.sinh()
		.atan();
	(WORLD_SIZE / n) * lat_rad.cos() / 256.0
}

fn parse_zxy(label: &str) -> (u32, u32, u32) {
	// label may be like "11/1034/709" or "11/1034/709.webp"
	let p: Vec<u32> = label
		.split('/')
		.map(|s| s.split('.').next().unwrap().parse().unwrap())
		.collect();
	(p[0], p[1], p[2])
}

// ── transforms (operate on a raw-grid in f64 elevation) ───────────────────────
type Grid = Vec<f64>; // elevation in metres, row-major w*h

fn zero_bits_for(tol_raw: f64) -> u32 {
	if tol_raw < 1.0 {
		0
	} else {
		(tol_raw + 1.0).log2().floor() as u32
	}
	.min(24)
}

fn quantize_raw<F: Fn(u32) -> u32>(elev: &Grid, f: F) -> Grid {
	elev
		.iter()
		.map(|&e| {
			let raw = ((e + 32768.0) / RAW_UNIT_M).round().clamp(0.0, 0x00FF_FFFF as f64) as u32;
			f(raw) as f64 * RAW_UNIT_M - 32768.0
		})
		.collect()
}

fn mask_transform(elev: &Grid, zero_bits: u32) -> Grid {
	let mask = if zero_bits == 0 {
		0x00FF_FFFF
	} else {
		0x00FF_FFFF & !((1u32 << zero_bits) - 1)
	};
	quantize_raw(elev, |raw| raw & mask)
}

fn rounded_mask_transform(elev: &Grid, zero_bits: u32) -> Grid {
	if zero_bits == 0 {
		return elev.clone();
	}
	let mask = 0x00FF_FFFF & !((1u32 << zero_bits) - 1);
	let half = 1u32 << (zero_bits - 1);
	quantize_raw(elev, |raw| (raw.saturating_add(half).min(0x00FF_FFFF)) & mask)
}

fn round_step_transform(elev: &Grid, q: u32) -> Grid {
	let q = q.max(1);
	quantize_raw(elev, |raw| {
		let qf = q as f64;
		((((raw as f64) / qf).round() * qf) as u32).min(0x00FF_FFFF)
	})
}

/// Clamped iterative box smoothing: pull each pixel toward its 3×3 mean but never
/// let it leave [orig-tol, orig+tol]. Drives toward the smoothest surface inside
/// the error tube → minimal high-frequency content for the entropy coder.
fn smooth_transform(elev: &Grid, w: usize, h: usize, tol: f64) -> Grid {
	let mut cur = elev.clone();
	let idx = |x: usize, y: usize| y * w + x;
	for _ in 0..SMOOTH_ITERS {
		let mut next = cur.clone();
		for y in 0..h {
			for x in 0..w {
				let (mut sum, mut cnt) = (0.0, 0.0);
				for dy in -1i32..=1 {
					for dx in -1i32..=1 {
						let nx = x as i32 + dx;
						let ny = y as i32 + dy;
						if nx >= 0 && ny >= 0 && (nx as usize) < w && (ny as usize) < h {
							sum += cur[idx(nx as usize, ny as usize)];
							cnt += 1.0;
						}
					}
				}
				let o = elev[idx(x, y)];
				next[idx(x, y)] = (sum / cnt).clamp(o - tol, o + tol);
			}
		}
		cur = next;
	}
	// snap to representable terrarium values
	cur.iter().map(|&e| rgb_to_elev_arr(elev_to_rgb(e))).collect()
}
fn rgb_to_elev_arr(p: [u8; 3]) -> f64 {
	rgb_to_elev(p[0], p[1], p[2])
}

// ── metrics ──────────────────────────────────────────────────────────────────
struct Err {
	max_e: f64,
	rms_e: f64,
	max_s: f64,
	rms_s: f64,
}

fn slope_deg(g: &Grid, w: usize, h: usize, x: usize, y: usize, pm: f64) -> f64 {
	let i = y * w + x;
	let dx = if x + 1 < w { (g[i + 1] - g[i]).abs() } else { 0.0 };
	let dy = if y + 1 < h { (g[i + w] - g[i]).abs() } else { 0.0 };
	(dx.max(dy) / pm).atan().to_degrees()
}

fn measure(orig: &Grid, new: &Grid, w: usize, h: usize, pm: f64) -> Err {
	let (mut max_e, mut sse, mut max_s, mut sss) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
	for y in 0..h {
		for x in 0..w {
			let i = y * w + x;
			let de = (new[i] - orig[i]).abs();
			max_e = max_e.max(de);
			sse += de * de;
			let ds = (slope_deg(new, w, h, x, y, pm) - slope_deg(orig, w, h, x, y, pm)).abs();
			max_s = max_s.max(ds);
			sss += ds * ds;
		}
	}
	let n = (w * h) as f64;
	Err {
		max_e,
		rms_e: (sse / n).sqrt(),
		max_s,
		rms_s: (sss / n).sqrt(),
	}
}

// ── webp size ────────────────────────────────────────────────────────────────
fn encode_size(grid: &Grid, w: i32, h: i32) -> usize {
	let mut rgb = Vec::with_capacity((w * h * 3) as usize);
	for &e in grid {
		rgb.extend_from_slice(&elev_to_rgb(e));
	}
	unsafe {
		let mut config = WebPConfig::new().unwrap();
		config.lossless = 1;
		config.method = 4;
		config.quality = 100.0;
		config.exact = 1;
		let mut picture = WebPPicture::new().unwrap();
		picture.use_argb = 1;
		picture.width = w;
		picture.height = h;
		if WebPPictureImportRGB(&raw mut picture, rgb.as_ptr(), w * 3) == 0 {
			WebPPictureFree(&raw mut picture);
			panic!("import failed");
		}
		let mut writer: WebPMemoryWriter = std::mem::zeroed();
		WebPMemoryWriterInit(&raw mut writer);
		picture.writer = Some(WebPMemoryWrite);
		picture.custom_ptr = (&raw mut writer).cast();
		let ok = WebPEncode(&raw const config, &raw mut picture);
		WebPPictureFree(&raw mut picture);
		if ok == 0 {
			WebPMemoryWriterClear(&raw mut writer);
			panic!("encode failed");
		}
		let size = writer.size;
		WebPFree(writer.mem.cast());
		size
	}
}

// Per-tile content-adaptive rounded quantization for a given slope budget: pick the
// largest power-of-two step whose REALISED errors satisfy both budgets.
fn adaptive_grid(orig: &Grid, w: usize, h: usize, pm: f64, tol_elev: f64, slope_budget_deg: f64) -> (Grid, u32) {
	// start from the geometric minimum derived from this slope budget
	let tol_slope = pm * slope_budget_deg.to_radians().tan();
	let tol_raw = tol_elev.min(tol_slope) / RAW_UNIT_M;
	let start = zero_bits_for(tol_raw);
	let mut bits = start.max(1).saturating_sub(2); // probe a little below, then climb
	let mut best = rounded_mask_transform(orig, bits);
	for cand_bits in (bits + 1)..=24 {
		let cand = rounded_mask_transform(orig, cand_bits);
		let e = measure(orig, &cand, w, h, pm);
		if e.max_e <= tol_elev && e.max_s <= slope_budget_deg {
			best = cand;
			bits = cand_bits;
		} else {
			break;
		}
	}
	(best, bits)
}

/// Quantize one elevation value with a rounded power-of-two step (zero_bits).
fn q_round_bits(e: f64, bits: u32) -> f64 {
	if bits == 0 {
		return e;
	}
	let mask = 0x00FF_FFFF & !((1u32 << bits) - 1);
	let half = 1u32 << (bits - 1);
	let raw = ((e + 32768.0) / RAW_UNIT_M).round().clamp(0.0, 0x00FF_FFFF as f64) as u32;
	(raw.saturating_add(half).min(0x00FF_FFFF) & mask) as f64 * RAW_UNIT_M - 32768.0
}

/// Spatially-adaptive (block-wise) rounded quantization: each block picks the largest
/// power-of-two step whose realised errors stay within budget for that block (boundary
/// checked against the unquantised outside neighbour, an approximation; the GLOBAL
/// measurement afterwards reveals any true boundary violations). Output stays valid
/// terrarium. Returns (grid, mean chosen bits per block).
fn block_adaptive_grid(
	orig: &Grid,
	w: usize,
	h: usize,
	pm: f64,
	tol_elev: f64,
	slope_budget: f64,
	block: usize,
) -> (Grid, f64) {
	let idx = |x: usize, y: usize| y * w + x;
	let mut out = orig.clone();
	let (mut bits_sum, mut nblocks) = (0.0f64, 0.0f64);

	let mut by = 0;
	while by < h {
		let mut bx = 0;
		while bx < w {
			let (x1, y1) = ((bx + block).min(w), (by + block).min(h));
			// climb bits while the whole block (incl. boundary to outside) stays in budget
			let mut chosen = 0u32;
			for bits in 1..=12u32 {
				let mut ok = true;
				'chk: for y in by..y1 {
					for x in bx..x1 {
						let o = orig[idx(x, y)];
						let qv = q_round_bits(o, bits);
						if (qv - o).abs() > tol_elev {
							ok = false;
							break 'chk;
						}
						for (nx, ny) in [(x + 1, y), (x, y + 1)] {
							if nx < w && ny < h {
								let on = orig[idx(nx, ny)];
								let inside = nx >= bx && nx < x1 && ny >= by && ny < y1;
								let qn = if inside { q_round_bits(on, bits) } else { on };
								let s0 = ((on - o).abs() / pm).atan().to_degrees();
								let s1 = ((qn - qv).abs() / pm).atan().to_degrees();
								if (s1 - s0).abs() > slope_budget {
									ok = false;
									break 'chk;
								}
							}
						}
					}
				}
				if ok {
					chosen = bits;
				} else {
					break;
				}
			}
			for y in by..y1 {
				for x in bx..x1 {
					out[idx(x, y)] = q_round_bits(orig[idx(x, y)], chosen);
				}
			}
			bits_sum += f64::from(chosen);
			nblocks += 1.0;
			bx += block;
		}
		by += block;
	}
	(out, bits_sum / nblocks)
}

fn main() {
	let images = load_tile_rgb_data();

	// Decode all tiles once.
	let tiles: Vec<(String, Grid, usize, usize, f64, f64)> = images
		.iter()
		.map(|(label, pixels, w, h)| {
			let (wu, hu) = (*w as usize, *h as usize);
			let (z, x, y) = parse_zxy(label);
			let pm = pixel_meters(z, x, y);
			let tol_elev = ELEVATION_ERROR * pm;
			let orig: Grid = (0..wu * hu)
				.map(|i| rgb_to_elev(pixels[i * 3], pixels[i * 3 + 1], pixels[i * 3 + 2]))
				.collect();
			(label.clone(), orig, wu, hu, pm, tol_elev)
		})
		.collect();

	let orig_total: usize = tiles
		.iter()
		.map(|(_, g, w, h, ..)| encode_size(g, *w as i32, *h as i32))
		.sum();

	// Baseline: the current op (truncating mask at the 1° slope budget).
	let mut base_total = 0usize;
	for (_, g, w, h, pm, tol_elev) in &tiles {
		let tol_slope = pm * SLOPE_ERROR_DEG.to_radians().tan();
		let zb = zero_bits_for(tol_elev.min(tol_slope) / RAW_UNIT_M);
		base_total += encode_size(&mask_transform(g, zb), *w as i32, *h as i32);
	}

	println!("Reference: orig (unquantized) = {orig_total} B; current op (mask, 1.0°) = {base_total} B\n");
	println!("slope_budget°\tsize\tvs_orig\tvs_current\tmaxΔm\trmsΔm\tmaxΔ°\trmsΔ°\tmean_bits");

	// Rate–quality curve: rounded, content-adaptive power-of-two quantization at several
	// slope budgets. (Rounding alone — same bits as current — is the 1.0° row vs current.)
	for &budget in &[1.0f64, 1.25, 1.5, 2.0, 3.0] {
		let (mut size, mut wmax_e, mut srms_e, mut wmax_s, mut srms_s, mut bits_sum) =
			(0usize, 0.0f64, 0.0f64, 0.0f64, 0.0f64, 0.0f64);
		for (_, g, w, h, pm, tol_elev) in &tiles {
			let (q, bits) = adaptive_grid(g, *w, *h, *pm, *tol_elev, budget);
			size += encode_size(&q, *w as i32, *h as i32);
			let e = measure(g, &q, *w, *h, *pm);
			wmax_e = wmax_e.max(e.max_e);
			srms_e += e.rms_e;
			wmax_s = wmax_s.max(e.max_s);
			srms_s += e.rms_s;
			bits_sum += f64::from(bits);
		}
		let n = tiles.len() as f64;
		println!(
			"{budget:.2}\t{size}\t{:+.1}%\t{:+.1}%\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.2}",
			(size as f64 / orig_total as f64 - 1.0) * 100.0,
			(size as f64 / base_total as f64 - 1.0) * 100.0,
			wmax_e,
			srms_e / n,
			wmax_s,
			srms_s / n,
			bits_sum / n,
		);
	}
	println!(
		"\n(method=4 lossless; {} tiles; rounded power-of-two, content-adaptive per tile)",
		tiles.len()
	);

	// ── spatially-adaptive (block-wise) at the conservative 1.0° budget ──────────
	println!("\nBlock-adaptive @ slope_budget=1.0° (size win WITHOUT relaxing quality):");
	println!("block\tsize\tvs_orig\tvs_current\tmaxΔm\trmsΔm\tmaxΔ°\trmsΔ°\tmean_bits");
	for &block in &[64usize, 32, 16, 8] {
		let (mut size, mut wmax_e, mut srms_e, mut wmax_s, mut srms_s, mut bits_sum) =
			(0usize, 0.0f64, 0.0f64, 0.0f64, 0.0f64, 0.0f64);
		for (_, g, w, h, pm, tol_elev) in &tiles {
			let (q, mean_bits) = block_adaptive_grid(g, *w, *h, *pm, *tol_elev, SLOPE_ERROR_DEG, block);
			size += encode_size(&q, *w as i32, *h as i32);
			let e = measure(g, &q, *w, *h, *pm);
			wmax_e = wmax_e.max(e.max_e);
			srms_e += e.rms_e;
			wmax_s = wmax_s.max(e.max_s);
			srms_s += e.rms_s;
			bits_sum += mean_bits;
		}
		let n = tiles.len() as f64;
		println!(
			"{block}²\t{size}\t{:+.1}%\t{:+.1}%\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.2}",
			(size as f64 / orig_total as f64 - 1.0) * 100.0,
			(size as f64 / base_total as f64 - 1.0) * 100.0,
			wmax_e,
			srms_e / n,
			wmax_s,
			srms_s / n,
			bits_sum / n,
		);
	}
	println!("(maxΔ° here is the GLOBAL realised slope error, including block boundaries)");
}
