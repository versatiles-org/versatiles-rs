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

use bench_common::{load_tile_rgb_data, original_blob_sizes};
use libwebp_sys::{
	WebPConfig, WebPEncode, WebPFree, WebPMemoryWrite, WebPMemoryWriter, WebPMemoryWriterClear, WebPMemoryWriterInit,
	WebPPicture, WebPPictureFree, WebPPictureImportRGB,
};

const WORLD_SIZE: f64 = 40_075_016.686;
const ELEVATION_ERROR: f64 = 0.1; // fraction of pixel ground size (matches dem_quantize default)
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
	encode_size_m(grid, w, h, 4)
}

fn encode_size_m(grid: &Grid, w: i32, h: i32, method: i32) -> usize {
	let mut rgb = Vec::with_capacity((w * h * 3) as usize);
	for &e in grid {
		rgb.extend_from_slice(&elev_to_rgb(e));
	}
	encode_rgb_bytes(&rgb, w, h, method)
}

/// Encode the grid with the blue channel forced to 0 (B plane becomes constant → ~free),
/// so `encode_size_m − encode_size_no_blue` is the blue channel's contribution to the file.
fn encode_size_no_blue(grid: &Grid, w: i32, h: i32, method: i32) -> usize {
	let mut rgb = Vec::with_capacity((w * h * 3) as usize);
	for &e in grid {
		let [r, g, _b] = elev_to_rgb(e);
		rgb.extend_from_slice(&[r, g, 0]);
	}
	encode_rgb_bytes(&rgb, w, h, method)
}

fn encode_rgb_bytes(rgb: &[u8], w: i32, h: i32, method: i32) -> usize {
	unsafe {
		let mut config = WebPConfig::new().unwrap();
		config.lossless = 1;
		config.method = method;
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

/// Replicates the production `dem_quantize` per-pixel adaptive algorithm (raw domain).
/// Returns (quantized grid, mean bits, sweeps used).
fn adaptive_production(orig: &Grid, w: usize, h: usize, pm: f64, ee: f64, se: f64) -> (Grid, f64, u32) {
	let to_raw = |e: f64| ((e + 32768.0) / RAW_UNIT_M).round().clamp(0.0, 0x00FF_FFFF as f64) as u32;
	let e_raw: Vec<u32> = orig.iter().map(|&e| to_raw(e)).collect();
	let n = e_raw.len();

	let tol_raw = (ee * pm) / RAW_UNIT_M;
	let init = if tol_raw < 1.0 {
		0u8
	} else {
		(tol_raw.log2().floor() as i32 + 1).clamp(0, 24) as u8
	};
	let q1 = |raw: u32, b: u8| -> u32 {
		if b == 0 {
			raw & 0x00FF_FFFF
		} else {
			let step = 1u32 << u32::from(b);
			(raw + (step >> 1)).min(0x00FF_FFFF) & (0x00FF_FFFF & !(step - 1))
		}
	};

	let mut bits = vec![init; n];
	let mut sweeps = 0u32;
	if init > 0 {
		let max_diff_raw = (pm * se.to_radians().tan()) / RAW_UNIT_M;
		for _ in 0..64 {
			sweeps += 1;
			let qerr: Vec<i64> = (0..n)
				.map(|i| i64::from(q1(e_raw[i], bits[i])) - i64::from(e_raw[i]))
				.collect();
			let mut dec = vec![false; n];
			let mut any = false;
			for y in 0..h {
				for x in 0..w {
					let i = y * w + x;
					if bits[i] == 0 {
						continue;
					}
					let qq = qerr[i];
					let over = |j: usize| (qq - qerr[j]).abs() as f64 > max_diff_raw;
					if (x + 1 < w && over(i + 1))
						|| (x > 0 && over(i - 1))
						|| (y + 1 < h && over(i + w))
						|| (y > 0 && over(i - w))
					{
						dec[i] = true;
						any = true;
					}
				}
			}
			if !any {
				break;
			}
			for (i, d) in dec.iter().enumerate() {
				if *d {
					bits[i] -= 1;
				}
			}
		}
	}
	let out: Grid = (0..n)
		.map(|i| f64::from(q1(e_raw[i], bits[i])) * RAW_UNIT_M - 32768.0)
		.collect();
	let mean_bits = bits.iter().map(|&b| f64::from(b)).sum::<f64>() / n as f64;
	(out, mean_bits, sweeps)
}

/// Option 1: total-variation flattening within the constraint "tube".
///
/// GOAL. Produce a new elevation `E'` per pixel that (1) stays within `elev_tol` of the
/// original `E` (elevation budget), (2) never changes the slope between two neighbours by
/// more than `slope_tol` (slope budget), and (3) is as *flat* as possible — long runs of
/// identical values — because WebP compresses runs almost for free.
///
/// KEY IDEA. Uniform quantization rounds every pixel to the grid *independently*, so two
/// neighbours that straddle a grid line end up one step apart (a residual WebP must store).
/// TV instead *coordinates* neighbours: it tries to make each pixel EQUAL to a neighbour we
/// already decided, extending a flat run, and only steps when the budgets force it. The
/// unused elevation budget is what lets a run drift along a gentle slope before it must step.
///
/// HOW THE BUDGETS BECOME AN INTERVAL. We sweep in raster order, so the left and top
/// neighbours are already final. For the pixel `p` we compute the set of values `E'[p]` that
/// still satisfy both budgets — a single interval `[lo, hi]`:
///   - Elevation box: `E'[p] ∈ [E[p] − elev_tol, E[p] + elev_tol]`.
///   - Slope to a decided neighbour `n`: we need the *change* in their difference to be small,
///     `|(E'[p] − E'[n]) − (E[p] − E[n])| ≤ slope_tol`. Writing `d = E[p] − E[n]` (the original
///     difference) and `c = E'[n] + d`, that is just `E'[p] ∈ [c − slope_tol, c + slope_tol]`.
///
/// Intersecting the box with the left- and top-slope intervals gives `[lo, hi]`.
///
/// WHY PREFERRING A NEIGHBOUR'S VALUE = FLATTENING. Setting `E'[p] = E'[left]` makes the new
/// left gradient 0, so the slope *change* there is `|0 − d| = |d|` — allowed exactly when the
/// original neighbours were within `slope_tol` (i.e. the terrain is locally gentle). That is
/// also precisely the condition `E'[left] ∈ [lo, hi]`. So "use the left value if it's in the
/// interval" means "flatten wherever the slope budget permits". Across a gentle ramp the run
/// value stays put while `E` rises; the *elevation box* is what finally forces a break, and
/// when it breaks the slope interval only allows a `≤ slope_tol` step — never a cliff.
fn tv_within_tube(orig: &Grid, w: usize, h: usize, pm: f64, ee: f64, se: f64) -> Grid {
	// Work in integer "raw" terrarium units (24-bit), so a value is exact and the grid is
	// integer multiples of `step`. (raw = R<<16 | G<<8 | B; 1 raw unit = 1/256 m.)
	let to_raw = |e: f64| ((e + 32768.0) / RAW_UNIT_M).round().clamp(0.0, 0x00FF_FFFF as f64) as i64;
	let e_raw: Vec<i64> = orig.iter().map(|&e| to_raw(e)).collect();

	// Both budgets expressed in raw units.
	let elev_tol = (ee * pm) / RAW_UNIT_M; // max |E' − E| per pixel
	let slope_tol = (pm * se.to_radians().tan()) / RAW_UNIT_M; // max change in an adjacent difference

	// `step` is the uniform-quantizer grid (same as the `uniform` path): the largest power-of-two
	// that fits the stricter budget. We prefer to land on multiples of it so the low bits stay 0.
	let zb = zero_bits_for(elev_tol.min(slope_tol));
	let step: i64 = if zb == 0 { 1 } else { 1i64 << zb };
	let step_f = step as f64;

	let mut ep = vec![0i64; e_raw.len()]; // the output values, filled in raster order
	for y in 0..h {
		for x in 0..w {
			let p = y * w + x;
			let er = e_raw[p] as f64;

			// Start with the elevation box, then tighten by the slope constraint to each
			// already-decided neighbour (left and top). Result: the feasible interval [lo, hi].
			let mut lo = er - elev_tol;
			let mut hi = er + elev_tol;
			if x > 0 {
				// slope interval from the left neighbour, centred at E'[left] + (E[p] − E[left])
				let c = ep[p - 1] as f64 + (e_raw[p] - e_raw[p - 1]) as f64;
				lo = lo.max(c - slope_tol);
				hi = hi.min(c + slope_tol);
			}
			if y > 0 {
				// slope interval from the top neighbour
				let c = ep[p - w] as f64 + (e_raw[p] - e_raw[p - w]) as f64;
				lo = lo.max(c - slope_tol);
				hi = hi.min(c + slope_tol);
			}

			// Choose a value inside [lo, hi], in order of how compressible it is:
			let val = if x > 0 && (ep[p - 1] as f64) >= lo && (ep[p - 1] as f64) <= hi {
				ep[p - 1] // 1. extend the horizontal run: reuse the left value (best for WebP)
			} else if y > 0 && (ep[p - w] as f64) >= lo && (ep[p - w] as f64) <= hi {
				ep[p - w] // 2. else match the pixel above (continue a vertical run)
			} else if lo <= hi {
				// 3. no neighbour fits, but the interval is non-empty: land on the grid multiple
				//    of `step` nearest the original (keeps low bits clean, like `uniform`).
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
					// 3b. interval narrower than the grid spacing → no multiple fits; take the
					//     clamped original. This is OFF-GRID (the only source of B-channel noise).
					t.round() as i64
				}
			} else {
				// 4. left and top slope demands conflict → empty interval (rare). Take the
				//    midpoint; this may slightly exceed a budget, which the error report exposes.
				f64::midpoint(lo, hi).round() as i64
			};
			ep[p] = val.clamp(0, 0x00FF_FFFF);
		}
	}

	// Back to elevation (metres). Note: every edge was checked against exactly one of its two
	// endpoints (the one processed later), so all edges end up within budget.
	ep.iter().map(|&r| r as f64 * RAW_UNIT_M - 32768.0).collect()
}

/// Combined (clean-B + valid): TV flattening snapped onto the uniform step grid, with a
/// per-tile slope margin. Snapping perturbs the slope, so TV is run at a tightened budget
/// `se*f`; we take the largest `f` whose snapped result still respects the real budget `se`.
/// Falls back to plain uniform (always clean + valid) if no margin works.
fn combined_within_tube(orig: &Grid, w: usize, h: usize, pm: f64, ee: f64, se: f64) -> Grid {
	let elev_tol = ee * pm / RAW_UNIT_M;
	let slope_tol = pm * se.to_radians().tan() / RAW_UNIT_M;
	let zb = zero_bits_for(elev_tol.min(slope_tol));
	if zb == 0 {
		return tv_within_tube(orig, w, h, pm, ee, se);
	}
	let step = 1i64 << zb;
	let step_f = step as f64;
	let snap = |g: &Grid| -> Grid {
		g.iter()
			.map(|&e| {
				let raw = ((e + 32768.0) / RAW_UNIT_M).round();
				((raw / step_f).round() as i64 * step).clamp(0, 0x00FF_FFFF) as f64 * RAW_UNIT_M - 32768.0
			})
			.collect()
	};
	let elev_tol_m = ee * pm;
	let valid = |g: &Grid| -> bool {
		for y in 0..h {
			for x in 0..w {
				let i = y * w + x;
				let qi = g[i] - orig[i];
				if qi.abs() > elev_tol_m + 1e-6 {
					return false;
				}
				if x + 1 < w && ((qi - (g[i + 1] - orig[i + 1])).abs() / pm).atan().to_degrees() > se + 1e-6 {
					return false;
				}
				if y + 1 < h && ((qi - (g[i + w] - orig[i + w])).abs() / pm).atan().to_degrees() > se + 1e-6 {
					return false;
				}
			}
		}
		true
	};
	for f in [1.0, 0.85, 0.7, 0.55, 0.4, 0.25] {
		let g = snap(&tv_within_tube(orig, w, h, pm, ee, se * f));
		if valid(&g) {
			return g;
		}
	}
	rounded_mask_transform(orig, zb)
}

#[allow(clippy::too_many_lines)]
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

	// ── Per-tile size table: original vs uniform (current op) vs TV ──────────────
	// elevation_error / slope_error at the dem_quantize defaults; WebP method 6 (production).
	let orig_sizes: std::collections::HashMap<String, usize> = original_blob_sizes().into_iter().collect();
	println!("Per-tile sizes (elevation_error={ELEVATION_ERROR}, slope_error={SLOPE_ERROR_DEG}°, WebP method 6):");
	println!("tile\toriginal\tuniform\tTV\tcombined\tTV_vs_uni\tcomb_vs_uni");
	let (mut o_tot, mut u_tot, mut t_tot, mut c_tot, mut best_tot) = (0usize, 0usize, 0usize, 0usize, 0usize);
	for (label, g, w, h, pm, _tol) in &tiles {
		let tol_slope = pm * SLOPE_ERROR_DEG.to_radians().tan();
		let zb = zero_bits_for((ELEVATION_ERROR * pm).min(tol_slope) / RAW_UNIT_M);
		let uni = encode_size_m(&rounded_mask_transform(g, zb), *w as i32, *h as i32, 6);
		let tv = encode_size_m(
			&tv_within_tube(g, *w, *h, *pm, ELEVATION_ERROR, SLOPE_ERROR_DEG),
			*w as i32,
			*h as i32,
			6,
		);
		let comb = encode_size_m(
			&combined_within_tube(g, *w, *h, *pm, ELEVATION_ERROR, SLOPE_ERROR_DEG),
			*w as i32,
			*h as i32,
			6,
		);
		let orig = *orig_sizes.get(label).unwrap_or(&0);
		o_tot += orig;
		u_tot += uni;
		t_tot += tv;
		c_tot += comb;
		best_tot += uni.min(comb);
		println!(
			"{label}\t{orig}\t{uni}\t{tv}\t{comb}\t{:+.1}%\t{:+.1}%",
			(tv as f64 / uni as f64 - 1.0) * 100.0,
			(comb as f64 / uni as f64 - 1.0) * 100.0
		);
	}
	println!(
		"TOTAL\t{o_tot}\t{u_tot}\t{t_tot}\t{c_tot}\t{:+.1}%\t{:+.1}%",
		(t_tot as f64 / u_tot as f64 - 1.0) * 100.0,
		(c_tot as f64 / u_tot as f64 - 1.0) * 100.0
	);
	println!(
		"best-of(uniform,combined) = {best_tot} ({:+.1}% vs uniform, {:+.1}% vs original)\n",
		(best_tot as f64 / u_tot as f64 - 1.0) * 100.0,
		(best_tot as f64 / o_tot as f64 - 1.0) * 100.0
	);

	// ── Blue-channel cost: full WebP vs WebP with B forced to 0, per version ──────
	// ΔB = full − no-blue = how many bytes the blue channel (fractional metre) costs.
	// "original" here = unquantized grid re-encoded at method 6 (not the downloaded blob).
	println!("Blue-channel cost (method 6; ΔB = full − no-blue bytes):");
	println!("tile\torig_full\torig_noB\torig_ΔB\tuni_full\tuni_noB\tuni_ΔB\ttv_full\ttv_noB\ttv_ΔB");
	let (mut ofs, mut onbs, mut ufs, mut unbs, mut tfs, mut tnbs) = (0usize, 0usize, 0usize, 0usize, 0usize, 0usize);
	for (label, g, w, h, pm, _tol) in &tiles {
		let tol_slope = pm * SLOPE_ERROR_DEG.to_radians().tan();
		let zb = zero_bits_for((ELEVATION_ERROR * pm).min(tol_slope) / RAW_UNIT_M);
		let uni = rounded_mask_transform(g, zb);
		let tvg = tv_within_tube(g, *w, *h, *pm, ELEVATION_ERROR, SLOPE_ERROR_DEG);
		let (wi, hi) = (*w as i32, *h as i32);
		let (of, onb) = (encode_size_m(g, wi, hi, 6), encode_size_no_blue(g, wi, hi, 6));
		let (uf, unb) = (encode_size_m(&uni, wi, hi, 6), encode_size_no_blue(&uni, wi, hi, 6));
		let (tf, tnb) = (encode_size_m(&tvg, wi, hi, 6), encode_size_no_blue(&tvg, wi, hi, 6));
		ofs += of;
		onbs += onb;
		ufs += uf;
		unbs += unb;
		tfs += tf;
		tnbs += tnb;
		println!(
			"{label}\t{of}\t{onb}\t{}\t{uf}\t{unb}\t{}\t{tf}\t{tnb}\t{}",
			of - onb,
			uf - unb,
			tf - tnb
		);
	}
	println!(
		"TOTAL\t{ofs}\t{onbs}\t{}\t{ufs}\t{unbs}\t{}\t{tfs}\t{tnbs}\t{}\n",
		ofs - onbs,
		ufs - unbs,
		tfs - tnbs
	);

	// ── Constraint verification: does the combined output obey both budgets? ──────
	println!("Combined constraint check (elev budget = {ELEVATION_ERROR}×pixel, slope budget = {SLOPE_ERROR_DEG}°):");
	println!("tile\tdistinctB\torig_min..max(m)\tout_min..max(m)\tmaxΔm\telev_budget_m\tmaxΔ°\tstatus");
	for (label, g, w, h, pm, _tol) in &tiles {
		let tv = combined_within_tube(g, *w, *h, *pm, ELEVATION_ERROR, SLOPE_ERROR_DEG);
		let mut raws: Vec<i64> = tv
			.iter()
			.map(|&e| ((e + 32768.0) / RAW_UNIT_M).round() as i64 & 0xFF)
			.collect();
		raws.sort_unstable();
		raws.dedup();
		let omin = g.iter().copied().fold(f64::INFINITY, f64::min);
		let omax = g.iter().copied().fold(f64::NEG_INFINITY, f64::max);
		let tmin = tv.iter().copied().fold(f64::INFINITY, f64::min);
		let tmax = tv.iter().copied().fold(f64::NEG_INFINITY, f64::max);
		let mut max_e = 0.0f64;
		let mut max_s = 0.0f64;
		for y in 0..*h {
			for x in 0..*w {
				let i = y * w + x;
				max_e = max_e.max((tv[i] - g[i]).abs());
				let qi = tv[i] - g[i];
				if x + 1 < *w {
					max_s = max_s.max(((qi - (tv[i + 1] - g[i + 1])).abs() / pm).atan().to_degrees());
				}
				if y + 1 < *h {
					max_s = max_s.max(((qi - (tv[i + w] - g[i + w])).abs() / pm).atan().to_degrees());
				}
			}
		}
		let elev_budget = ELEVATION_ERROR * pm;
		let status = if max_e <= elev_budget + 1e-3 && max_s <= SLOPE_ERROR_DEG + 1e-3 {
			"PASS"
		} else {
			"FAIL"
		};
		println!(
			"{label}\t{}\t{omin:.0}..{omax:.0}\t{tmin:.0}..{tmax:.0}\t{max_e:.1}\t{elev_budget:.0}\t{max_s:.3}\t{status}",
			raws.len()
		);
	}
	println!();

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

	// ── Option 1: best-of(uniform, TV) vs elevation_error (slope_error fixed at 1.0°) ─
	// Sweep elevation_error to see how the TV gain and the realised elevation deviation
	// shrink as the budget is tightened. "vs current" is against the shipped op
	// (rounded uniform at elevation_error=0.5). All at method 4.
	let ref_current: usize = tiles
		.iter()
		.map(|(_, g, w, h, pm, _)| {
			let tol_slope = pm * SLOPE_ERROR_DEG.to_radians().tan();
			let zb = zero_bits_for((0.5 * pm).min(tol_slope) / RAW_UNIT_M);
			encode_size(&rounded_mask_transform(g, zb), *w as i32, *h as i32)
		})
		.sum();
	println!("\nOption 1 — best-of(uniform,TV) vs elevation_error (slope_error=1.0°, method=4):");
	println!("elev_err\tuniform\tbest-of\tbestof_vs_uni\tbestof_vs_current\tmaxΔm\trmsΔm\tmaxΔ°");
	for &ee in &[0.5f64, 0.25, 0.1, 0.05, 0.02] {
		let (mut uni_size, mut best_size) = (0usize, 0usize);
		let (mut b_me, mut b_re, mut b_ms) = (0.0f64, 0.0f64, 0.0f64);
		for (_, g, w, h, pm, _) in &tiles {
			let tol_slope = pm * SLOPE_ERROR_DEG.to_radians().tan();
			let zb = zero_bits_for((ee * pm).min(tol_slope) / RAW_UNIT_M);
			let uni = rounded_mask_transform(g, zb);
			let tv = tv_within_tube(g, *w, *h, *pm, ee, SLOPE_ERROR_DEG);
			let us = encode_size(&uni, *w as i32, *h as i32);
			let ts = encode_size(&tv, *w as i32, *h as i32);
			uni_size += us;
			// best-of: keep the smaller, and report ITS realised errors
			let (chosen, csz) = if ts < us { (&tv, ts) } else { (&uni, us) };
			best_size += csz;
			let e = measure(g, chosen, *w, *h, *pm);
			b_me = b_me.max(e.max_e);
			b_re += e.rms_e;
			b_ms = b_ms.max(e.max_s);
		}
		let n = tiles.len() as f64;
		println!(
			"{ee:.2}\t{uni_size}\t{best_size}\t{:+.1}%\t{:+.1}%\t{b_me:.2}\t{:.2}\t{b_ms:.3}",
			(best_size as f64 / uni_size as f64 - 1.0) * 100.0,
			(best_size as f64 / ref_current as f64 - 1.0) * 100.0,
			b_re / n,
		);
	}

	// ── WebP effort sweep on the FIXED current-op output (no quality change) ──────
	// Same quantized pixels (current rounded-uniform op) → only the encoder effort
	// changes. This is the one transparent lever left within the current constraints.
	println!("\nWebP effort on current-op output (identical pixels, identical errors):");
	println!("method\tsize\tvs_m4");
	let quantized: Vec<(Grid, i32, i32)> = tiles
		.iter()
		.map(|(_, g, w, h, pm, tol_elev)| {
			let tol_slope = pm * SLOPE_ERROR_DEG.to_radians().tan();
			let zb = zero_bits_for(tol_elev.min(tol_slope) / RAW_UNIT_M);
			(rounded_mask_transform(g, zb), *w as i32, *h as i32)
		})
		.collect();
	let mut m4_total = 0usize;
	for &method in &[4i32, 5, 6] {
		let size: usize = quantized.iter().map(|(g, w, h)| encode_size_m(g, *w, *h, method)).sum();
		if method == 4 {
			m4_total = size;
		}
		println!(
			"{method}\t{size}\t{:+.2}%",
			(size as f64 / m4_total as f64 - 1.0) * 100.0
		);
	}

	// ── production per-pixel adaptive algorithm (matches dem_quantize.rs) ─────────
	println!("\nProduction per-pixel adaptive (elevation_error=0.5, slope_error=1.0°):");
	println!("method\tsize\tvs_orig\tvs_current\tmaxΔm\trmsΔm\tmaxΔ°\trmsΔ°\tmean_bits\tmax_sweeps");
	{
		let (mut size, mut wmax_e, mut srms_e, mut wmax_s, mut srms_s, mut bits_sum, mut max_sweeps) =
			(0usize, 0.0f64, 0.0f64, 0.0f64, 0.0f64, 0.0f64, 0u32);
		for (_, g, w, h, pm, _tol) in &tiles {
			let (q, mean_bits, sweeps) = adaptive_production(g, *w, *h, *pm, ELEVATION_ERROR, SLOPE_ERROR_DEG);
			size += encode_size(&q, *w as i32, *h as i32);
			let e = measure(g, &q, *w, *h, *pm);
			wmax_e = wmax_e.max(e.max_e);
			srms_e += e.rms_e;
			wmax_s = wmax_s.max(e.max_s);
			srms_s += e.rms_s;
			bits_sum += mean_bits;
			max_sweeps = max_sweeps.max(sweeps);
		}
		let n = tiles.len() as f64;
		println!(
			"adaptive\t{size}\t{:+.1}%\t{:+.1}%\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.2}\t{max_sweeps}",
			(size as f64 / orig_total as f64 - 1.0) * 100.0,
			(size as f64 / base_total as f64 - 1.0) * 100.0,
			wmax_e,
			srms_e / n,
			wmax_s,
			srms_s / n,
			bits_sum / n,
		);
	}
}
