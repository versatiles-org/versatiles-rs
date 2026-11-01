use anyhow::{Result, anyhow, bail, ensure};
use versatiles_image::color::HexColor;

/// The ramp `ramp=default` stands for: a hypsometric tint, light and muted so
/// that a hillshade on top of it stays legible. The coast lies just above 0,
/// because DEMs without bathymetry, such as Mapterhorn's, store the sea as
/// exactly 0 m.
pub const DEFAULT_RAMP: &str = "-11000:24507e,-200:8cb4d8,0:bcd8ec,\
	0.01:a8c896,300:cad6a2,800:e4dcae,1500:d6bf98,2500:bea58a,4000:e4e0dc,6000:ffffff";

/// Colours for elevations, as a list of `elevation:colour` stops, or `default`
/// for [`DEFAULT_RAMP`].
///
/// Carries the format of `ramp=` in its type, so that a bad stop is judged
/// where a value is decoded, including by `check` (#257).
///
/// Between two stops the colour is interpolated linearly, and outside the
/// ramp it is the colour of the nearest end, as in MapLibre's `color-relief`
/// layer. MapLibre interpolates premultiplied colours, which keeps a fade to
/// a transparent stop from taking on that stop's hidden colour; so does this.
#[derive(Clone, Debug, PartialEq)]
pub struct ColorRamp {
	/// Ascending by elevation, at least one.
	stops: Vec<Stop>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Stop {
	meters: f64,
	/// Premultiplied RGBA in 0..=1.
	color: [f64; 4],
}

impl ColorRamp {
	/// Whether every colour the ramp gives is opaque. Since the ends hold
	/// outside it, that is whether every stop is.
	#[must_use]
	pub fn is_opaque(&self) -> bool {
		self.stops.iter().all(|stop| stop.color[3] >= 1.0)
	}

	/// The colour for `meters`, as straight RGBA8.
	#[must_use]
	pub fn color_at(&self, meters: f64) -> [u8; 4] {
		unpremultiply(self.premultiplied_at(meters))
	}

	/// The colour for `meters`, as premultiplied RGBA in 0..=1, for drawing
	/// something over it before it is rounded to 8 bits.
	#[must_use]
	pub fn premultiplied_at(&self, meters: f64) -> [f64; 4] {
		let after = self.stops.partition_point(|stop| stop.meters <= meters);
		match (after.checked_sub(1).map(|i| &self.stops[i]), self.stops.get(after)) {
			(Some(below), Some(above)) => {
				let t = (meters - below.meters) / (above.meters - below.meters);
				[0, 1, 2, 3].map(|k| below.color[k] + (above.color[k] - below.color[k]) * t)
			}
			(Some(end), None) | (None, Some(end)) => end.color,
			(None, None) => unreachable!("a ramp has at least one stop"),
		}
	}
}

/// A premultiplied colour in 0..=1 as straight RGBA8.
fn unpremultiply([r, g, b, a]: [f64; 4]) -> [u8; 4] {
	#[expect(
		clippy::cast_possible_truncation,
		clippy::cast_sign_loss,
		reason = "clamped to 0..=255"
	)]
	let byte = |v: f64| (v * 255.0).round().clamp(0.0, 255.0) as u8;
	if a <= 0.0 {
		return [0, 0, 0, 0];
	}
	[byte(r / a), byte(g / a), byte(b / a), byte(a)]
}

impl TryFrom<&str> for ColorRamp {
	type Error = anyhow::Error;

	fn try_from(text: &str) -> Result<Self> {
		if text.trim() == "default" {
			return ColorRamp::try_from(DEFAULT_RAMP);
		}
		let mut stops: Vec<Stop> = Vec::new();
		for part in text.split(',').map(str::trim).filter(|part| !part.is_empty()) {
			let (meters, color) = part
				.split_once(':')
				.ok_or_else(|| anyhow!("stop '{part}' is not written as 'elevation:colour'"))?;
			let meters: f64 = meters
				.trim()
				.parse()
				.map_err(|_| anyhow!("stop '{part}' does not start with an elevation in meters"))?;
			ensure!(meters.is_finite(), "stop '{part}' needs a finite elevation");
			let color = HexColor::try_from(color.trim()).map_err(|e| anyhow!("stop '{part}': {e}"))?;
			if let Some(previous) = stops.last() {
				ensure!(
					meters > previous.meters,
					"the stops must ascend by elevation, but {meters} follows {}",
					previous.meters
				);
			}
			let channels = color.channels();
			let [r, g, b] = [0, 1, 2].map(|i| f64::from(channels[i]) / 255.0);
			let a = channels.get(3).map_or(1.0, |&a| f64::from(a) / 255.0);
			stops.push(Stop {
				meters,
				color: [r * a, g * a, b * a, a],
			});
		}
		if stops.is_empty() {
			bail!("a colour ramp needs at least one 'elevation:colour' stop");
		}
		Ok(ColorRamp { stops })
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn ramp(text: &str) -> ColorRamp {
		ColorRamp::try_from(text).unwrap()
	}

	#[test]
	fn interpolates_between_stops_and_holds_the_ends_outside() {
		let ramp = ramp("0:000000, 100:FF8000");
		assert_eq!(ramp.color_at(-50.0), [0, 0, 0, 255]);
		assert_eq!(ramp.color_at(0.0), [0, 0, 0, 255]);
		assert_eq!(ramp.color_at(50.0), [128, 64, 0, 255]);
		assert_eq!(ramp.color_at(100.0), [255, 128, 0, 255]);
		assert_eq!(ramp.color_at(9000.0), [255, 128, 0, 255]);
	}

	#[test]
	fn a_single_stop_colours_everything() {
		let ramp = ramp("0:123456");
		assert_eq!(ramp.color_at(-1e4), [0x12, 0x34, 0x56, 255]);
		assert_eq!(ramp.color_at(1e4), [0x12, 0x34, 0x56, 255]);
	}

	/// Halfway from transparent red to opaque blue is half-transparent blue,
	/// not purple: the red of a stop nobody sees does not bleed in.
	#[test]
	fn colours_are_interpolated_premultiplied() {
		let ramp = ramp("0:FF000000,100:0000FF");
		assert_eq!(ramp.color_at(50.0), [0, 0, 255, 128]);
		assert_eq!(ramp.color_at(-1.0), [0, 0, 0, 0]);
	}

	#[test]
	fn opacity_is_judged_by_value_not_by_spelling() {
		assert!(ramp("0:000000,1:FFFFFF").is_opaque());
		assert!(ramp("0:000000FF,1:#FFF").is_opaque());
		assert!(!ramp("0:000000,1:FFFFFFFE").is_opaque());
	}

	#[test]
	fn the_spellings_hex_colours_have_are_accepted() {
		assert_eq!(ramp("-10.5:#F00, 20:00FF00").color_at(-10.5), [255, 0, 0, 255]);
		assert_eq!(ramp(" 0 : 0000FF80 ,").color_at(0.0), [0, 0, 255, 128]);
	}

	#[test]
	fn default_stands_for_the_default_ramp() {
		assert_eq!(ramp("default"), ramp(DEFAULT_RAMP));
		assert!(ramp("default").is_opaque());
	}

	#[test]
	fn bad_ramps_are_refused_with_a_reason() {
		for (text, reason) in [
			("Default", "elevation:colour"),
			("", "at least one"),
			(" , ", "at least one"),
			("100", "elevation:colour"),
			("high:FFFFFF", "elevation in meters"),
			("NaN:FFFFFF", "finite"),
			("0:red", "hex digit"),
			("100:000000,0:FFFFFF", "ascend"),
			("0:000000,0:FFFFFF", "ascend"),
		] {
			let error = ColorRamp::try_from(text).unwrap_err().to_string();
			assert!(error.contains(reason), "{text:?}: {error}");
		}
	}
}
