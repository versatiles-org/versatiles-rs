//! Rendering a cell's id from the coordinates of its lower-left corner.
//!
//! Published grids key their tables on an id built from those coordinates, and
//! every publisher builds it differently — the axis order, the unit the numbers
//! are in, whether they are zero-padded, and what prefixes them all vary:
//!
//! | Source | Id | Template |
//! |---|---|---|
//! | Eurostat GISCO / INSPIRE | `CRS3035RES1000mN2691000E4341000` | `CRS3035RES1000mN{y}E{x}` |
//! | GEOSTAT short form | `1kmN2689E4337` | `1kmN{y/1000}E{x/1000}` |
//! | CBS Netherlands | `E0643N4567` | `E{x/100:04}N{y/100:04}` |
//! | Statistics Finland | `250mN674400E31725` | `250mN{y/10}E{x/10}` |
//!
//! So the id is a template of literal text and `{x}` / `{y}` placeholders, each
//! taking an optional divisor (`{x/1000}`) and an optional zero-padded width
//! (`{x:04}`, `{x/100:04}`).
//!
//! Negative coordinates are published — Eurostat's own 100 km grid contains
//! `CRS3035RES100000mN-2800000E8700000` for the French overseas territories — so
//! they render with a leading `-`, and padding applies to the digits after it.

use std::fmt::Write as _;

use anyhow::{Context, Result, bail, ensure};

/// Which coordinate a placeholder renders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Axis {
	X,
	Y,
}

#[derive(Debug, Clone)]
enum Part {
	Literal(String),
	Coordinate { axis: Axis, divisor: f64, width: usize },
}

/// A parsed cell id template.
#[derive(Debug, Clone)]
pub struct IdTemplate {
	parts: Vec<Part>,
	source: String,
}

impl IdTemplate {
	/// Parse a template of literal text with `{x}` / `{y}` placeholders.
	pub fn parse(template: &str) -> Result<Self> {
		let mut parts = Vec::new();
		let mut literal = String::new();
		let mut rest = template;

		while let Some(open) = rest.find('{') {
			literal.push_str(&rest[..open]);
			let after = &rest[open + 1..];
			let close = after
				.find('}')
				.with_context(|| format!("unclosed '{{' in id template '{template}'"))?;

			if !literal.is_empty() {
				parts.push(Part::Literal(std::mem::take(&mut literal)));
			}
			parts.push(parse_placeholder(&after[..close], template)?);
			rest = &after[close + 1..];
		}
		literal.push_str(rest);
		if !literal.is_empty() {
			parts.push(Part::Literal(literal));
		}

		ensure!(
			parts.iter().any(|p| matches!(p, Part::Coordinate { .. })),
			"id template '{template}' has no {{x}} or {{y}} placeholder, so every cell would get the same id"
		);

		Ok(Self {
			parts,
			source: template.to_string(),
		})
	}

	/// The INSPIRE form, as Eurostat, its member states and the Eurostat
	/// GridMaker publish it: `CRS3035RES1000mN{y}E{x}`.
	pub fn inspire(epsg: u32, size: f64) -> Result<Self> {
		let size = integral_size(size, "the `inspire` id preset")?;
		Self::parse(&format!("CRS{epsg}RES{size}mN{{y}}E{{x}}"))
	}

	/// The GEOSTAT short form, coordinates in units of the cell size:
	/// `1kmN{y/1000}E{x/1000}`.
	pub fn geostat(size: f64) -> Result<Self> {
		let integral = integral_size(size, "the `geostat` id preset")?;
		let label = if integral % 1000 == 0 {
			format!("{}km", integral / 1000)
		} else {
			format!("{integral}m")
		};
		Self::parse(&format!("{label}N{{y/{integral}}}E{{x/{integral}}}"))
	}

	/// Check that every divisor divides the grid itself.
	///
	/// A divisor the cell size is not a multiple of would round two different
	/// cells onto one id, which shows up as rows that mysteriously fail to join
	/// rather than as an error — so it is refused up front.
	pub fn validate_for(&self, size: f64, offset: [f64; 2]) -> Result<()> {
		for part in &self.parts {
			let Part::Coordinate { axis, divisor, .. } = part else {
				continue;
			};
			if (*divisor - 1.0).abs() < f64::EPSILON {
				continue;
			}

			let axis_name = if *axis == Axis::X { "x" } else { "y" };
			let offset = if *axis == Axis::X { offset[0] } else { offset[1] };
			ensure!(
				(size % divisor).abs() < f64::EPSILON,
				"id template '{}' divides {axis_name} by {divisor}, but the cell size {size} is not a multiple of it, \
				 so neighbouring cells would share an id",
				self.source
			);
			ensure!(
				(offset % divisor).abs() < f64::EPSILON,
				"id template '{}' divides {axis_name} by {divisor}, but the grid offset {offset} is not a multiple of \
				 it, so ids would not be whole numbers",
				self.source
			);
		}
		Ok(())
	}

	/// The id of the cell whose lower-left corner is at `x` / `y`.
	#[must_use]
	pub fn render(&self, x: f64, y: f64) -> String {
		let mut out = String::new();
		for part in &self.parts {
			match part {
				Part::Literal(text) => out.push_str(text),
				Part::Coordinate { axis, divisor, width } => {
					let value = if *axis == Axis::X { x } else { y } / divisor;
					// Cell corners are exact multiples of the divisor by the time
					// `validate_for` has passed, so rounding only removes the noise
					// a division leaves behind.
					let value = value.round();
					if value < 0.0 {
						out.push('-');
					}
					#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
					let magnitude = value.abs() as u64;
					// Infallible: writing into a String.
					let _ = write!(out, "{magnitude:0width$}", width = *width);
				}
			}
		}
		out
	}
}

fn parse_placeholder(body: &str, template: &str) -> Result<Part> {
	let (before_width, width) = match body.split_once(':') {
		Some((before, width)) => {
			let width = width
				.parse::<usize>()
				.with_context(|| format!("width '{width}' in id template '{template}' is not a number"))?;
			(before, width)
		}
		None => (body, 0),
	};

	let (axis, divisor) = match before_width.split_once('/') {
		Some((axis, divisor)) => {
			let divisor = divisor
				.parse::<f64>()
				.with_context(|| format!("divisor '{divisor}' in id template '{template}' is not a number"))?;
			ensure!(
				divisor > 0.0,
				"divisor '{divisor}' in id template '{template}' must be greater than zero"
			);
			(axis, divisor)
		}
		None => (before_width, 1.0),
	};

	let axis = match axis.trim() {
		"x" => Axis::X,
		"y" => Axis::Y,
		other => bail!("unknown placeholder '{{{other}}}' in id template '{template}': only {{x}} and {{y}} exist"),
	};

	Ok(Part::Coordinate { axis, divisor, width })
}

/// Presets spell the cell size into the id, so it has to be a whole number.
fn integral_size(size: f64, who: &str) -> Result<i64> {
	ensure!(
		size.fract().abs() < f64::EPSILON && size > 0.0,
		"{who} writes the cell size into every id, so it has to be a whole number of CRS units, but size is {size}. \
		 Use `id_template=` to spell the id out instead."
	);
	#[allow(clippy::cast_possible_truncation)]
	Ok(size as i64)
}

#[cfg(test)]
mod tests {
	use super::*;

	/// Every row is an id copied from a published dataset.
	#[test]
	fn renders_ids_that_real_datasets_use() {
		let cases: &[(&str, f64, f64, &str)] = &[
			// Eurostat GISCO / INSPIRE, 1 km.
			(
				"CRS3035RES1000mN{y}E{x}",
				4_341_000.0,
				2_691_000.0,
				"CRS3035RES1000mN2691000E4341000",
			),
			// The same grid at 100 km, where Eurostat publishes negative northings.
			(
				"CRS3035RES100000mN{y}E{x}",
				8_700_000.0,
				-2_800_000.0,
				"CRS3035RES100000mN-2800000E8700000",
			),
			// GEOSTAT short form: coordinates in cell-size units.
			("1kmN{y/1000}E{x/1000}", 4_337_000.0, 2_689_000.0, "1kmN2689E4337"),
			// CBS Netherlands: easting first, hectometers, padded to four digits.
			("E{x/100:04}N{y/100:04}", 64_300.0, 456_700.0, "E0643N4567"),
			// Statistics Finland.
			("250mN{y/10}E{x/10}", 317_250.0, 6_744_000.0, "250mN674400E31725"),
		];

		for (template, x, y, expected) in cases {
			let rendered = IdTemplate::parse(template).unwrap().render(*x, *y);
			assert_eq!(&rendered, expected, "template {template}");
		}
	}

	#[test]
	fn presets_match_their_published_form() {
		assert_eq!(
			IdTemplate::inspire(3035, 1000.0)
				.unwrap()
				.render(4_341_000.0, 2_691_000.0),
			"CRS3035RES1000mN2691000E4341000"
		);
		assert_eq!(
			IdTemplate::geostat(1000.0).unwrap().render(4_337_000.0, 2_689_000.0),
			"1kmN2689E4337"
		);
		assert_eq!(
			IdTemplate::geostat(250.0).unwrap().render(317_250.0, 6_744_000.0),
			"250mN26976E1269"
		);
	}

	#[test]
	fn padding_counts_digits_not_the_sign() {
		let template = IdTemplate::parse("N{y:05}").unwrap();
		assert_eq!(template.render(0.0, 42.0), "N00042");
		assert_eq!(template.render(0.0, -42.0), "N-00042");
		assert_eq!(template.render(0.0, 1_234_567.0), "N1234567");
	}

	#[test]
	fn a_divisor_the_grid_does_not_share_is_refused() {
		let template = IdTemplate::parse("{x/1000}").unwrap();
		// A 1 km grid divided by 1000 is fine...
		template.validate_for(1000.0, [0.0, 0.0]).unwrap();
		// ...a 100 m grid is not: ten cells would share every id.
		let err = template.validate_for(100.0, [0.0, 0.0]).unwrap_err().to_string();
		assert!(err.contains("would share an id"), "{err}");
		// ...and neither is an offset that is not a multiple.
		let err = template.validate_for(1000.0, [500.0, 0.0]).unwrap_err().to_string();
		assert!(err.contains("not a multiple"), "{err}");
	}

	#[test]
	fn malformed_templates_are_refused() {
		let err = IdTemplate::parse("cell_{z}").unwrap_err().to_string();
		assert!(err.contains("only {x} and {y}"), "{err}");

		let err = IdTemplate::parse("cell_{x").unwrap_err().to_string();
		assert!(err.contains("unclosed"), "{err}");

		let err = IdTemplate::parse("plain text").unwrap_err().to_string();
		assert!(err.contains("no {x} or {y}"), "{err}");

		let err = IdTemplate::parse("{x/0}").unwrap_err().to_string();
		assert!(err.contains("greater than zero"), "{err}");

		let err = IdTemplate::parse("{x:wide}").unwrap_err().to_string();
		assert!(err.contains("is not a number"), "{err}");
	}

	#[test]
	fn a_preset_needs_a_whole_number_size() {
		let err = IdTemplate::inspire(3035, 0.01).unwrap_err().to_string();
		assert!(err.contains("whole number"), "{err}");
	}
}
