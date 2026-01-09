use crate::TileBBox;
use std::fmt;

/// Implements `Debug` for [`TileBBox`].
///
/// The output shows zoom level, inclusive coordinates, and dimensions:
///
/// ```text
/// z: [x_min,y_min,x_max,y_max] (widthxheight)
/// ```
///
/// Example:
/// ```
/// # use versatiles_core::TileBBox;
/// let bb = TileBBox::from_min_and_size(4, 5, 6, 3, 2).unwrap();
/// assert_eq!(format!("{:?}", bb), "4: [5,6,7,7] (3x2)");
/// ```
impl fmt::Debug for TileBBox {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(
			f,
			"{}: [{},{},{},{}] ({}x{})",
			self.level,
			self.x_min().map_or(String::from("?"), |v| v.to_string()),
			self.y_min().map_or(String::from("?"), |v| v.to_string()),
			self.x_max().map_or(String::from("?"), |v| v.to_string()),
			self.y_max().map_or(String::from("?"), |v| v.to_string()),
			self.width(),
			self.height()
		)
	}
}

/// Implements `Display` for [`TileBBox`].
///
/// The output includes only zoom level and inclusive coordinates without dimensions:
///
/// ```text
/// z:[x_min,y_min,x_max,y_max]
/// ```
///
/// Example:
/// ```
/// # use versatiles_core::TileBBox;
/// let bb = TileBBox::from_min_and_size(4, 5, 6, 3, 2).unwrap();
/// assert_eq!(format!("{}", bb), "4:[5,6,7,7]");
/// ```
impl fmt::Display for TileBBox {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(
			f,
			"{}:[{},{},{},{}]",
			self.level,
			self.x_min().map_or(String::from("?"), |v| v.to_string()),
			self.y_min().map_or(String::from("?"), |v| v.to_string()),
			self.x_max().map_or(String::from("?"), |v| v.to_string()),
			self.y_max().map_or(String::from("?"), |v| v.to_string())
		)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use anyhow::Result;
	use rstest::rstest;

	#[rstest]
	// z=0 full world (1x1)
	#[case(0, (0,0,0,0), "0:[0,0,0,0]", "0: [0,0,0,0] (1x1)")]
	// a 3x2 box at z=4 starting at (5,6)
	#[case(4, (5,6,7,7), "4:[5,6,7,7]", "4: [5,6,7,7] (3x2)")]
	// a single tile at higher zoom
	#[case(10, (512,768,512,768), "10:[512,768,512,768]", "10: [512,768,512,768] (1x1)")]
	fn display_and_debug_formats(
		#[case] level: u8,
		#[case] minmax: (u32, u32, u32, u32),
		#[case] expect_display: &str,
		#[case] expect_debug: &str,
	) -> Result<()> {
		let (x0, y0, x1, y1) = minmax;
		let bb = TileBBox::from_min_and_max(level, x0, y0, x1, y1)?;

		// Display
		let s_disp = format!("{bb}");
		assert_eq!(s_disp, expect_display);

		// Debug
		let s_dbg = format!("{bb:?}");
		assert_eq!(s_dbg, expect_debug);
		Ok(())
	}

	#[test]
	fn debug_and_display_consistency_width_height() -> Result<()> {
		// Sanity: 6x4 box should reflect (6x4) in Debug output
		let bb = TileBBox::from_min_and_size(5, 10, 20, 6, 4)?;
		assert_eq!(bb.width(), 6);
		assert_eq!(bb.height(), 4);
		let s_dbg = format!("{bb:?}");
		assert!(s_dbg.ends_with(" (6x4)"), "debug string lacks correct size: {s_dbg}");
		Ok(())
	}
}
