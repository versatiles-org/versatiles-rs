//! Utilities for generating and analyzing **synthetic test images** used in the `versatiles_image` crate.
//!
//! This module defines [`DynamicImageTraitTest`], which extends [`image::DynamicImage`] with deterministic
//! factory methods that generate reproducible 256×256 pixel patterns. These patterns are used across unit tests
//! to validate encoding, decoding, and image-processing behavior. They help detect subtle channel swaps,
//! alignment issues, or rounding errors between formats.
//!
//! # Marker-based regression testing
//!
//! The functions [`new_marker`] and [`gauge_marker`] generate and measure oriented gradient markers that encode
//! a simple linear model:
//!
//! ```text
//! value(x,y) = scale * (cos(angle) * x + sin(angle) * y - offset) + 128
//! ```
//!
//! The image generator takes one or more [`MarkerParameters`] specifying `offset`, `angle`, and `scale`, producing
//! synthetic directional gradients for each channel. The analyzer then fits a regression plane per channel and
//! estimates the same parameters as [`MarkerResult`].
//!
//! This mechanism allows robust roundtrip testing of linear image content (e.g., after re-encoding or reprojection)
//! while ignoring saturated or clipped pixels. Only the **offset projection** along the line’s direction is
//! identifiable; perpendicular shifts cannot be reconstructed from pixel intensities alone.

use super::convert::DynamicImageTraitConvert;
use anyhow::{Result, bail, ensure};
use image::{DynamicImage, GenericImageView};
use versatiles_derive::context;

/// Describes a synthetic directional gradient marker for one image channel.
/// Used by [`DynamicImageTraitTest::new_marker`] to generate test images.
///
/// The gradient is modeled as:
/// `value(x, y) = scale * (cos(angle) * x + sin(angle) * y - offset) + 128`
///
/// * `offset` – translation of the line pattern along its direction.
/// * `angle` – orientation of the gradient line in **degrees**.
/// * `scale` – slope magnitude (contrast strength), normalized to 1/256.
#[derive(Clone, Copy, Debug)]
pub struct MarkerParameters {
	/// Offset along the marker direction (projection of the translation on the angle).
	pub offset: f64,
	/// Direction angle in **degrees**.
	pub angle: f64,
	/// Scale (slope magnitude) of the marker line.
	pub scale: f64,
}

impl MarkerParameters {
	pub fn new(offset: f64, angle: f64, scale: f64) -> Self {
		Self { offset, angle, scale }
	}
}

/// Estimated gradient parameters recovered from an image channel.
/// Produced by [`DynamicImageTraitTest::gauge_marker`].
///
/// * `offset` – recovered translation along the direction.
/// * `angle` – detected gradient orientation in **degrees**.
/// * `scale` – recovered slope magnitude, normalized to 1/256.
/// * `error` – mean absolute residual from the regression fit.
#[derive(Clone, Copy, Debug)]
pub struct MarkerResult {
	pub offset: f64,
	pub angle: f64,
	pub scale: f64,
	pub error: f64,
}

impl MarkerResult {
	#[context("comparing marker result (factor={:.3}) to expected (offset={:.3}, angle={:.1}, scale={:.3})", factor, p.offset, p.angle, p.scale)]
	pub fn compare(&self, p: &MarkerParameters, factor: f64) -> Result<()> {
		fn angle_delta(a: f64, b: f64) -> f64 {
			// Normalize to [-180°, 180°)
			let mut d = (a - b).rem_euclid(360.0);
			if d >= 180.0 {
				d -= 360.0;
			}
			// principal-axis equivalence (θ ≡ θ ± 180°)
			if d.abs() > 90.0 {
				d = 180.0 - d.abs();
			}
			d.abs()
		}

		let mut errors = vec![];
		if (p.offset - self.offset).abs() > 0.11 * factor {
			errors.push(format!(
				" - offset mismatch: expected {:.3}, got {:.3} (Δ={:.3})",
				p.offset,
				self.offset,
				(p.offset - self.offset).abs()
			));
		}
		if angle_delta(p.angle, self.angle).abs() > 0.4 * factor {
			errors.push(format!(
				" - angle mismatch: expected {:.1}, got {:.1} (Δ={:.1})",
				p.angle,
				self.angle,
				angle_delta(p.angle, self.angle)
			));
		}
		if (p.scale - self.scale).abs() > 0.1 * factor {
			errors.push(format!(
				" - scale mismatch: expected {:.3}, got {:.3} (Δ={:.3})",
				p.scale,
				self.scale,
				(p.scale - self.scale).abs()
			));
		}
		if self.error > 1.0 * factor {
			errors.push(format!(" - high residual error: {:.3}", self.error));
		}
		if !errors.is_empty() {
			bail!("{}", errors.join("\n"));
		}
		Ok(())
	}
}

#[context("comparing {} channel marker results", params.len())]
pub fn compare_marker_result(params: &[MarkerParameters], results: &[MarkerResult]) -> Result<()> {
	ensure!(
		params.len() == results.len(),
		"parameter/result count mismatch: expected {}, got {}",
		params.len(),
		results.len()
	);
	for (i, (p, r)) in params.iter().zip(results.iter()).enumerate() {
		if let Err(errors) = r.compare(p, 1.0) {
			bail!("error in channel {}:\n{}", i + 1, errors);
		}
	}
	Ok(())
}

/// Provides factory functions for generating reproducible gradient-based test images.
/// These are useful for validating conversions, encoders, and format roundtrips.
pub trait DynamicImageTraitTest: DynamicImageTraitConvert {
	/// Generates a 256×256 image with **RGBA** channels.
	/// Red increases with x, green decreases with x, blue increases with y, and alpha decreases with y.
	fn new_test_rgba() -> DynamicImage;

	/// Generates a 256×256 image with **RGB** channels.
	/// Red increases with x, green decreases with x, and blue increases with y.
	fn new_test_rgb() -> DynamicImage;

	/// Generates a 256×256 **grayscale** image.
	/// The brightness increases linearly along the x-axis (from black to white).
	fn new_test_grey() -> DynamicImage;

	/// Generates a 256×256 **grayscale + alpha (LA8)** image.
	/// The luminance increases with x, and the alpha increases with y.
	fn new_test_greya() -> DynamicImage;

	fn new_marker(parameters: &[MarkerParameters]) -> DynamicImage;
	fn gauge_marker(&self) -> Vec<MarkerResult>;
}

impl DynamicImageTraitTest for DynamicImage
where
	DynamicImage: DynamicImageTraitConvert,
{
	fn new_test_rgba() -> DynamicImage {
		#[allow(clippy::cast_possible_truncation)]
		DynamicImage::from_fn(256, 256, |x, y| [x as u8, (255 - x) as u8, y as u8, (255 - y) as u8])
	}

	fn new_test_rgb() -> DynamicImage {
		#[allow(clippy::cast_possible_truncation)]
		DynamicImage::from_fn(256, 256, |x, y| [x as u8, (255 - x) as u8, y as u8])
	}

	fn new_test_grey() -> DynamicImage {
		#[allow(clippy::cast_possible_truncation)]
		DynamicImage::from_fn(256, 256, |x, _y| [x as u8])
	}

	fn new_test_greya() -> DynamicImage {
		#[allow(clippy::cast_possible_truncation)]
		DynamicImage::from_fn(256, 256, |x, y| [x as u8, y as u8])
	}

	/// Generates a synthetic multi-channel marker image.
	/// Each channel encodes a directional gradient defined by the provided [`MarkerParameters`].
	/// Values are centered around 128 and clipped to [0, 255].
	fn new_marker(parameters: &[MarkerParameters]) -> DynamicImage {
		fn f<const N: usize>(x: u32, y: u32, parameters: &[MarkerParameters; N]) -> [u8; N] {
			let xf = f64::from(x) - 128.0;
			let yf = f64::from(y) - 128.0;

			#[allow(clippy::cast_possible_truncation)]
			parameters.map(|p| {
				let angle_rad = p.angle.to_radians();
				let v = angle_rad.cos() * xf + angle_rad.sin() * yf - p.offset;
				(v * p.scale / 256.0 + 128.0).round().clamp(0.0, 255.0) as u8
			})
		}

		match parameters.len() {
			1 => {
				let p = [parameters[0]];
				DynamicImage::from_fn(256, 256, |x, y| f(x, y, &p))
			}
			2 => {
				let p = [parameters[0], parameters[1]];
				DynamicImage::from_fn(256, 256, |x, y| f(x, y, &p))
			}
			3 => {
				let p = [parameters[0], parameters[1], parameters[2]];
				DynamicImage::from_fn(256, 256, |x, y| f(x, y, &p))
			}
			4 => {
				let p = [parameters[0], parameters[1], parameters[2], parameters[3]];
				DynamicImage::from_fn(256, 256, |x, y| f(x, y, &p))
			}
			_ => panic!("new_marker supports only 1 to 4 channels"),
		}
	}

	/// Measures each channel of the image and recovers its gradient parameters
	/// using a least-squares regression fit to `v ≈ a*x + b*y + c`.
	///
	/// Returns one [`MarkerResult`] per channel.
	fn gauge_marker(&self) -> Vec<MarkerResult> {
		let (width, height) = self.dimensions();
		let mut results = Vec::new();
		for c in 0..self.color().channel_count() {
			// Accumulate unweighted normal-equation terms for linear regression
			// v ≈ a*x + b*y + c, where
			//   a = scale * cos(theta),
			//   b = scale * sin(theta),
			//   c = -(a*dx + b*dy).
			let mut s_x = 0.0;
			let mut s_y = 0.0;
			let mut s_1 = 0.0;
			let mut s_xx = 0.0;
			let mut s_xy = 0.0;
			let mut s_yy = 0.0;
			let mut s_xv = 0.0;
			let mut s_yv = 0.0;
			let mut s_v = 0.0;

			for y in 0..height {
				for x in 0..width {
					let b = self.get_raw_pixel(x, y)[c as usize] as f64;
					// Ignore saturated pixels; those are clipped by generation and non-linear
					if b <= 0.0 || b >= 255.0 {
						continue;
					}
					let v = b - 128.0;
					let xf = f64::from(x) - width as f64 / 2.0;
					let yf = f64::from(y) - height as f64 / 2.0;

					s_x += xf;
					s_y += yf;
					s_1 += 1.0;
					s_xx += xf * xf;
					s_xy += xf * yf;
					s_yy += yf * yf;
					s_xv += xf * v;
					s_yv += yf * v;
					s_v += v;
				}
			}

			// Solve the 3x3 normal equations:
			// [Sxx Sxy Sx] [a] = [Sxv]
			// [Sxy Syy Sy] [b]   [Syv]
			// [Sx  Sy  S1] [c]   [Sv ]
			let det = s_xx * (s_yy * s_1 - s_y * s_y) - s_xy * (s_xy * s_1 - s_x * s_y) + s_x * (s_xy * s_y - s_yy * s_x);
			// Fallback: if determinant is tiny (e.g., empty due to all-saturated), skip channel
			if !det.is_finite() || det.abs() < 1e-9 {
				results.push(MarkerResult {
					offset: 0.0,
					angle: 0.0,
					scale: 0.0,
					error: f64::INFINITY,
				});
				continue;
			}
			let det_a =
				s_xv * (s_yy * s_1 - s_y * s_y) - s_xy * (s_yv * s_1 - s_x * s_v) + s_x * (s_yv * s_y - s_yy * s_v);
			let det_b =
				s_xx * (s_yv * s_1 - s_x * s_v) - s_xv * (s_xy * s_1 - s_x * s_y) + s_x * (s_xy * s_v - s_yv * s_x);
			let det_c =
				s_xx * (s_yy * s_v - s_yv * s_y) - s_xy * (s_xy * s_v - s_yv * s_x) + s_x * (s_xy * s_yv - s_yy * s_xv);

			let a = det_a / det;
			let b2 = det_b / det; // avoid shadowing `b` from img pixel
			let c_hat = det_c / det;

			// Recover angle and scale from a,b
			let angle = b2.atan2(a).to_degrees();
			let scale = (a * a + b2 * b2).sqrt() * 256.0;

			// The identifiable translation is its projection along the direction:
			// offset = (a*dx + b*dy) / scale = -(c)/scale
			let offset = -c_hat / (scale / 256.0);

			// Estimate error as average absolute residual
			let mut error_sum = 0.0;
			let mut n = 0.0;
			for y in 0..height {
				for x in 0..width {
					let bb = self.get_raw_pixel(x, y)[c as usize] as f64;
					if bb <= 0.0 || bb >= 255.0 {
						continue;
					}
					let v = bb - 128.0;
					let xf = f64::from(x) - width as f64 / 2.0;
					let yf = f64::from(y) - height as f64 / 2.0;
					let v_hat = a * xf + b2 * yf + c_hat;
					error_sum += (v - v_hat).abs();
					n += 1.0;
				}
			}
			let error = if n > 0.0 { error_sum / n } else { f64::INFINITY };

			results.push(MarkerResult {
				offset,
				angle,
				scale,
				error,
			});
		}
		results
	}
}

/// Unit tests verifying pixel gradients and expected value patterns for each synthetic image.
/// The test compares selected pixel values (0, 128, 255) to symbolic representations for clarity.
#[cfg(test)]
mod tests {
	use super::*;
	use rstest::rstest;
	use versatiles_derive::context;

	/// Helper: run `MarkerResult::compare` expecting failure and return the error message string.
	fn compare_err_msg(p: MarkerParameters, r: MarkerResult, factor: f64) -> String {
		r.compare(&p, factor).unwrap_err().chain().last().unwrap().to_string()
	}

	/// Verifies that each synthetic test image (grey, greya, rgb, rgba)
	/// produces the expected gradient pattern and channel alignment.
	#[rstest]
	#[case::grey(DynamicImage::new_test_grey(), [
		"...# +++# ####",
		"...# +++# ####",
		"...# +++# ####"
	])]
	#[case::greya(DynamicImage::new_test_greya(), [
		".... +++. ###.",
		"...+ ++++ ###+",
		"...# +++# ####"
	])]
	#[case::rgb(DynamicImage::new_test_rgb(), [
		".#.# ++.# #..#",
		".#+# +++# #.+#",
		".### ++## #.##"
	])]
	#[case::rgba(DynamicImage::new_test_rgba(), [
		".#.# ++.# #..#",
		".#++ ++++ #.++",
		".##. ++#. #.#."
	])]
	fn check_dimensions_and_gradients(#[case] img: DynamicImage, #[case] colors: [&str; 3]) {
		assert_eq!(img.dimensions(), (256, 256));
		let get_pixel = |x: u32, y: u32| {
			img.get_pixel(x, y)
				.0
				.iter()
				.map(|v| match v {
					0 => '.',
					127 | 128 => '+',
					255 => '#',
					_ => panic!("unexpected value {v}"),
				})
				.collect::<String>()
		};
		let colors_result = [
			[get_pixel(0, 0), get_pixel(128, 0), get_pixel(255, 0)].join(" "),
			[get_pixel(0, 128), get_pixel(128, 128), get_pixel(255, 128)].join(" "),
			[get_pixel(0, 255), get_pixel(128, 255), get_pixel(255, 255)].join(" "),
		];
		assert_eq!(colors_result, colors);
	}

	/// Roundtrip the marker generator+gauge for 1–4 channel combinations.
	#[rstest]
	#[case::grey([ ( 7,  21, 85)])]
	#[case::greya([(10, -23, 90), (-6,  63, 70)])]
	#[case::rgb([  ( 3,  14, 80), (-7, -54, 65), (12, 80, 75)])]
	#[case::rgba([ ( 4,  34, 88), (-9, -68, 67), (15, 60, 72), (-20, -17, 93)])]
	fn marker_gauge_roundtrip<const N: usize>(#[case] args: [(i32, i32, i32); N]) {
		let params = args.map(|(offset, angle, scale)| MarkerParameters::new(offset as f64, angle as f64, scale as f64));
		let img = DynamicImage::new_marker(&params);
		assert_eq!(img.dimensions(), (256, 256));
		assert_eq!(img.color().channel_count() as usize, N);
		let results = img.gauge_marker();
		compare_marker_result(&params, &results).unwrap();
	}

	#[test]
	#[context("test: compare tolerates exact thresholds")]
	fn compare_tolerates_exact_thresholds() -> Result<()> {
		// Offsets: allowed Δ <= 0.11 * factor
		let p = MarkerParameters {
			offset: 10.0,
			angle: 30.0,
			scale: 80.0,
		};
		let r = MarkerResult {
			offset: 10.11,
			angle: 30.4,
			scale: 80.10,
			error: 1.0,
		};
		r.compare(&p, 1.0)
	}

	#[test]
	fn compare_fails_just_over_thresholds() {
		let p = MarkerParameters {
			offset: 10.0,
			angle: 30.0,
			scale: 80.0,
		};
		// Each term barely exceeds its threshold
		let r = MarkerResult {
			offset: 10.11001,
			angle: 30.4001,
			scale: 80.1001,
			error: 1.00001,
		};
		let msg = compare_err_msg(p, r, 1.0);
		assert!(msg.contains("offset mismatch"), "msg: {msg}");
		assert!(msg.contains("angle mismatch"), "msg: {msg}");
		assert!(msg.contains("scale mismatch"), "msg: {msg}");
		assert!(msg.contains("high residual error"), "msg: {msg}");
	}

	#[test]
	fn compare_respects_factor_scaling() {
		let p = MarkerParameters {
			offset: 0.0,
			angle: 0.0,
			scale: 100.0,
		};
		// With factor 2.0, thresholds double; this should pass
		let r = MarkerResult {
			offset: 0.21,
			angle: 0.79,
			scale: 100.19,
			error: 1.99,
		};
		r.compare(&p, 2.0).expect("should pass with scaled thresholds");
		// But with factor 1.0, the same would fail
		let msg = compare_err_msg(p, r, 1.0);
		assert!(
			msg.contains("offset mismatch")
				|| msg.contains("angle mismatch")
				|| msg.contains("scale mismatch")
				|| msg.contains("high residual error")
		);
	}

	#[test]
	#[context("test: compare angle wrap & principal-axis equivalence")]
	fn compare_angle_wrap_and_principal_axis_equivalence() -> Result<()> {
		// 179° vs -181° is effectively 0° difference after wrap; also principal-axis equivalence
		let p = MarkerParameters {
			offset: 0.0,
			angle: 179.0,
			scale: 50.0,
		};
		// -181° normalizes to 179°, delta ~0°; should be accepted
		let r = MarkerResult {
			offset: 0.0,
			angle: -181.0,
			scale: 50.0,
			error: 0.1,
		};
		r.compare(&p, 1.0)?;

		// 170° vs -10° is a 180° flip; principal-axis equivalence should see 10° delta
		// Within 0.4 threshold when factor large enough
		let p2 = MarkerParameters {
			offset: 0.0,
			angle: 170.0,
			scale: 50.0,
		};
		let r2 = MarkerResult {
			offset: 0.0,
			angle: -10.0,
			scale: 50.0,
			error: 0.1,
		};
		// With factor 30, threshold = 12°, so 10° is ok
		r2.compare(&p2, 30.0)
	}
}
