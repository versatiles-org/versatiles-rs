//! Rendering a cell's id from the coordinates of its lower-left corner.
//!
//! Published grids key their tables on an id built from those coordinates, and
//! every publisher builds it differently — the axis order, the unit the numbers
//! are in, whether they are zero-padded, and what prefixes them all vary:
//!
//! | Source | Id | Template |
//! |---|---|---|
//! | Eurostat GISCO / INSPIRE | `CRS3035RES1000mN2691000E4341000` | `CRS{epsg}RES{size}m{y:h}{x:h}` |
//! | GEOSTAT short form | `1kmN2689E4337` | `{size/1000}km{y/1000:h}{x/1000:h}` |
//! | CBS Netherlands | `E0643N4567` | `{x/100:h04}{y/100:h04}` |
//! | Statistics Finland | `250mN674400E31725` | `{size}m{y/10:h}{x/10:h}` |
//!
//! So the id is literal text plus placeholders:
//!
//! ```text
//! { name [ /divisor ] [ : [sign] [width] ] }
//! ```
//!
//! `name` is `x` or `y` for the corner, or `epsg` / `size` for the grid's own
//! arguments — the latter two so that an id naming the grid cannot drift from
//! the grid it names. `divisor` puts the number in another unit (`{x/1000}`),
//! and `width` zero-pads it (`{x/100:04}`).
//!
//! `sign` decides how the coordinate's direction is written:
//!
//! | flag | `2691000` | `-2800000` |
//! |---|---|---|
//! | none, or `-` | `2691000` | `-2800000` |
//! | `+` | `+2691000` | `-2800000` |
//! | `h` | `N2691000` | `S2800000` |
//! | `H` | `2691000N` | `2800000S` |
//!
//! `h` and `H` write N/S for `y` and E/W for `x` and render the magnitude, which
//! is what makes the letter mean something: a literal `N` in the template stays
//! `N` south of the origin, and Eurostat's own grid reaches there —
//! `CRS3035RES100000mN-2800000E8700000` is a French overseas cell as the literal
//! form renders it. Padding always counts digits, never the sign or the letter.

use std::fmt::Write as _;

use anyhow::{Context, Result, bail, ensure};

/// Which coordinate a placeholder renders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Axis {
	X,
	Y,
}

/// How a coordinate's direction is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sign {
	/// `-` before a negative number, nothing before a positive one.
	Minus,
	/// Always a sign: `+2691000`, `-2800000`.
	Plus,
	/// A hemisphere letter before the magnitude: `N2691000`, `S2800000`.
	LetterFirst,
	/// A hemisphere letter after it: `2691000N`, `2800000S`.
	LetterLast,
}

impl Sign {
	/// The letter this axis uses on each side of the origin.
	fn letters(axis: Axis) -> (char, char) {
		match axis {
			Axis::X => ('E', 'W'),
			Axis::Y => ('N', 'S'),
		}
	}
}

#[derive(Debug, Clone)]
enum Part {
	Literal(String),
	Coordinate {
		axis: Axis,
		divisor: f64,
		width: usize,
		sign: Sign,
	},
}

/// A parsed cell id template.
#[derive(Debug, Clone)]
pub struct IdTemplate {
	parts: Vec<Part>,
	source: String,
}

impl IdTemplate {
	/// Parse a template of literal text with `{x}` / `{y}` placeholders.
	pub fn parse(template: &str, epsg: u32, size: f64) -> Result<Self> {
		let mut parts = Vec::new();
		let mut literal = String::new();
		let mut rest = template;

		while let Some(open) = rest.find('{') {
			literal.push_str(&rest[..open]);
			let after = &rest[open + 1..];
			let close = after
				.find('}')
				.with_context(|| format!("unclosed '{{' in id template '{template}'"))?;

			match parse_placeholder(&after[..close], template, epsg, size)? {
				// `{epsg}` and `{size}` are the same for every cell, so they fold
				// into the surrounding text instead of being rendered per id.
				Parsed::Literal(text) => literal.push_str(&text),
				Parsed::Coordinate(part) => {
					if !literal.is_empty() {
						parts.push(Part::Literal(std::mem::take(&mut literal)));
					}
					parts.push(part);
				}
			}
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
				Part::Coordinate {
					axis,
					divisor,
					width,
					sign,
				} => {
					let value = if *axis == Axis::X { x } else { y } / divisor;
					// Cell corners are exact multiples of the divisor by the time
					// `validate_for` has passed, so rounding only removes the noise
					// a division leaves behind.
					let value = value.round();
					let (positive, negative) = Sign::letters(*axis);
					// Zero belongs to the positive side: one cell sits on the axis,
					// and it needs one id.
					let letter = if value < 0.0 { negative } else { positive };

					match sign {
						Sign::Minus => {
							if value < 0.0 {
								out.push('-');
							}
						}
						Sign::Plus => out.push(if value < 0.0 { '-' } else { '+' }),
						Sign::LetterFirst => out.push(letter),
						// The letter follows the digits; written below.
						Sign::LetterLast => {}
					}

					#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
					let magnitude = value.abs() as u64;
					// Infallible: writing into a String.
					let _ = write!(out, "{magnitude:0width$}", width = *width);

					if *sign == Sign::LetterLast {
						out.push(letter);
					}
				}
			}
		}
		out
	}
}

/// What one placeholder turned out to be.
enum Parsed {
	/// `{epsg}` / `{size}`: the same text in every id.
	Literal(String),
	/// `{x}` / `{y}`: rendered per cell.
	Coordinate(Part),
}

fn parse_placeholder(body: &str, template: &str, epsg: u32, size: f64) -> Result<Parsed> {
	let (before_format, format) = match body.split_once(':') {
		Some((before, format)) => (before, format),
		None => (body, ""),
	};

	// The sign flag, if any, comes before the width: `{y:h04}`.
	let (sign, width) = match format.chars().next() {
		Some('-') => (Sign::Minus, &format[1..]),
		Some('+') => (Sign::Plus, &format[1..]),
		Some('h') => (Sign::LetterFirst, &format[1..]),
		Some('H') => (Sign::LetterLast, &format[1..]),
		_ => (Sign::Minus, format),
	};
	let width = if width.is_empty() {
		0
	} else {
		width
			.parse::<usize>()
			.with_context(|| format!("width '{width}' in id template '{template}' is not a number"))?
	};

	let (name, divisor) = match before_format.split_once('/') {
		Some((name, divisor)) => {
			let divisor = divisor
				.parse::<f64>()
				.with_context(|| format!("divisor '{divisor}' in id template '{template}' is not a number"))?;
			ensure!(
				divisor > 0.0,
				"divisor '{divisor}' in id template '{template}' must be greater than zero"
			);
			(name, divisor)
		}
		None => (before_format, 1.0),
	};

	let axis = match name.trim() {
		"x" => Axis::X,
		"y" => Axis::Y,
		// The grid's own arguments, so an id naming them cannot contradict them.
		"epsg" => {
			return Ok(Parsed::Literal(render_constant(
				f64::from(epsg),
				divisor,
				width,
				template,
				"epsg",
			)?));
		}
		"size" => {
			return Ok(Parsed::Literal(render_constant(
				size, divisor, width, template, "size",
			)?));
		}
		other => bail!(
			"unknown placeholder '{{{other}}}' in id template '{template}': the names are {{x}}, {{y}}, {{epsg}} and \
			 {{size}}"
		),
	};

	Ok(Parsed::Coordinate(Part::Coordinate {
		axis,
		divisor,
		width,
		sign,
	}))
}

/// Render `{epsg}` or `{size}`, which have to come out as whole numbers.
///
/// An id spelling a fractional cell size — `RES0.5m` — would be a label no
/// publisher uses and no join would match, so it is refused rather than rounded.
fn render_constant(value: f64, divisor: f64, width: usize, template: &str, name: &str) -> Result<String> {
	let value = value / divisor;
	ensure!(
		value.fract().abs() < f64::EPSILON,
		"id template '{template}' writes {{{name}}} into every id, so it has to be a whole number, but it is {value}. \
		 Divide it — `{{{name}/1000}}` — or spell the label out as literal text."
	);
	#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
	let magnitude = value.abs() as u64;
	Ok(format!("{}{magnitude:0width$}", if value < 0.0 { "-" } else { "" }))
}

#[cfg(test)]
mod tests {
	use super::*;

	/// Every row is an id copied from a published dataset.
	#[test]
	fn renders_ids_that_real_datasets_use() {
		// (template, epsg, size, x, y, expected)
		let cases: &[(&str, u32, f64, f64, f64, &str)] = &[
			// Eurostat GISCO / INSPIRE, 1 km — the default template.
			(
				"CRS{epsg}RES{size}m{y:h}{x:h}",
				3035,
				1000.0,
				4_341_000.0,
				2_691_000.0,
				"CRS3035RES1000mN2691000E4341000",
			),
			// GEOSTAT short form: coordinates in cell-size units, size as a label.
			(
				"{size/1000}km{y/1000:h}{x/1000:h}",
				3035,
				1000.0,
				4_337_000.0,
				2_689_000.0,
				"1kmN2689E4337",
			),
			// CBS Netherlands: easting first, hectometers, padded to four digits.
			(
				"{x/100:h04}{y/100:h04}",
				28992,
				100.0,
				64_300.0,
				456_700.0,
				"E0643N4567",
			),
			// Statistics Finland.
			(
				"{size}m{y/10:h}{x/10:h}",
				3067,
				250.0,
				317_250.0,
				6_744_000.0,
				"250mN674400E31725",
			),
		];

		for (template, epsg, size, x, y, expected) in cases {
			let rendered = IdTemplate::parse(template, *epsg, *size).unwrap().render(*x, *y);
			assert_eq!(&rendered, expected, "template {template}");
		}
	}

	/// A hemisphere letter has to follow the coordinate, not sit beside it.
	///
	/// Eurostat's own 100 km grid reaches the southern hemisphere — the French
	/// overseas territories — and a literal `N` in the template stays `N` there,
	/// which is how `CRS3035RES100000mN-2800000E8700000` comes about. The `h`
	/// flag is what makes the letter mean something.
	#[test]
	fn hemisphere_letters_follow_the_sign() {
		let template = IdTemplate::parse("{y:h}{x:h}", 3035, 100_000.0).unwrap();
		assert_eq!(template.render(8_700_000.0, 2_800_000.0), "N2800000E8700000");
		assert_eq!(template.render(8_700_000.0, -2_800_000.0), "S2800000E8700000");
		assert_eq!(template.render(-8_700_000.0, -2_800_000.0), "S2800000W8700000");
		// Zero belongs to the positive side: the cell on the axis needs one id.
		assert_eq!(template.render(0.0, 0.0), "N0E0");

		// The literal form, for comparison — the letter and the sign disagree.
		let literal = IdTemplate::parse("N{y}E{x}", 3035, 100_000.0).unwrap();
		assert_eq!(literal.render(8_700_000.0, -2_800_000.0), "N-2800000E8700000");
	}

	/// Every sign style, on the same coordinate.
	#[test]
	fn sign_styles_render_as_documented() {
		let render = |flag: &str, value: f64| {
			IdTemplate::parse(&format!("{{y:{flag}}}"), 3035, 1000.0)
				.unwrap()
				.render(0.0, value)
		};
		for (flag, positive, negative) in [
			("", "2691000", "-2800000"),
			("-", "2691000", "-2800000"),
			("+", "+2691000", "-2800000"),
			("h", "N2691000", "S2800000"),
			("H", "2691000N", "2800000S"),
		] {
			assert_eq!(render(flag, 2_691_000.0), positive, "flag {flag:?}");
			assert_eq!(render(flag, -2_800_000.0), negative, "flag {flag:?}");
		}
	}

	/// A flag and a width combine, and the width counts digits only.
	#[test]
	fn padding_counts_digits_not_the_sign_or_the_letter() {
		let plain = IdTemplate::parse("{y:05}", 3035, 1.0).unwrap();
		assert_eq!(plain.render(0.0, 42.0), "00042");
		assert_eq!(plain.render(0.0, -42.0), "-00042");
		assert_eq!(plain.render(0.0, 1_234_567.0), "1234567");

		let lettered = IdTemplate::parse("{y:h05}{x:H03}", 3035, 1.0).unwrap();
		assert_eq!(lettered.render(-7.0, -42.0), "S00042007W");
	}

	/// `{epsg}` and `{size}` come from the operation, so an id cannot name a
	/// grid it is not on.
	#[test]
	fn the_grid_constants_render_as_whole_numbers() {
		let template = IdTemplate::parse("CRS{epsg}RES{size}m{y}", 3035, 1000.0).unwrap();
		assert_eq!(template.render(0.0, 1.0), "CRS3035RES1000m1");

		// A fractional label is a join key nobody publishes, so it is refused
		// rather than rounded into something plausible.
		let err = IdTemplate::parse("RES{size}m{y}", 3035, 0.01).unwrap_err().to_string();
		assert!(err.contains("whole number"), "{err}");

		// Dividing it is how a label in another unit is spelled.
		let template = IdTemplate::parse("{size/1000}km{y}", 3035, 1000.0).unwrap();
		assert_eq!(template.render(0.0, 1.0), "1km1");
	}

	#[test]
	fn a_divisor_the_grid_does_not_share_is_refused() {
		let template = IdTemplate::parse("{x/1000}", 3035, 1000.0).unwrap();
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
		let parse = |t: &str| IdTemplate::parse(t, 3035, 1000.0).unwrap_err().to_string();

		let err = parse("cell_{z}");
		assert!(err.contains("{x}, {y}, {epsg} and {size}"), "{err}");

		let err = parse("cell_{x");
		assert!(err.contains("unclosed"), "{err}");

		let err = parse("plain text");
		assert!(err.contains("no {x} or {y}"), "{err}");

		let err = parse("{x/0}");
		assert!(err.contains("greater than zero"), "{err}");

		let err = parse("{x:wide}");
		assert!(err.contains("is not a number"), "{err}");

		// A template naming only the constants has the same id for every cell.
		let err = parse("CRS{epsg}RES{size}m");
		assert!(err.contains("no {x} or {y}"), "{err}");
	}
}
