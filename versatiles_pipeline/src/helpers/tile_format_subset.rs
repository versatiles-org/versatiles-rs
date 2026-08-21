//! Narrow tile-format types for parameters that accept only some of [`TileFormat`].
//!
//! `VPLDecode` renders a parameter's accepted values from its type's
//! `variants()`, so a parameter typed [`TileFormat`] advertises every format the
//! core knows — `bin`, `json`, `svg`, `topojson` included — even when the
//! operation can only produce a handful. These types make the parameter's type
//! say what the operation actually takes.
//!
//! That is not only a documentation fix. A value outside the real set used to be
//! caught late or not at all: `from_debug format=geojson` built happily and
//! emitted tiles with `tile_type: unknown`, and `from_gdal_raster
//! tile_format=geojson` reached an `expect("tile_format is raster")`. Typing the
//! parameter moves the rejection into [`check`](crate::check), which needs no
//! I/O and can run on a keystroke.

use anyhow::{Result, bail};
use versatiles_core::TileFormat;

/// Declares a `TileFormat` subset with the pieces `VPLDecode` and the operations need:
/// `TryFrom<&str>` to parse a VPL value, `variants()` for the generated
/// reference and the TypeScript bindings, and `Into<TileFormat>` to hand back to
/// the rest of the pipeline.
///
/// Aliases (`"jpeg"` for `jpg`) parse but are deliberately absent from
/// `variants()`, which lists the canonical spelling only.
macro_rules! tile_format_subset {
	(
		$(#[$meta:meta])*
		$name:ident { $( $variant:ident => $canonical:literal $(| $alias:literal)* => $format:ident ),+ $(,)? }
	) => {
		$(#[$meta])*
		#[derive(Debug, Clone, Copy, PartialEq, Eq)]
		pub enum $name {
			$($variant),+
		}

		impl $name {
			/// The accepted VPL spellings, canonical form only.
			///
			/// The single source of truth for this parameter's accepted values:
			/// `VPLDecode` renders these into the generated parameter reference,
			/// so no doc comment repeats them. Kept in step with
			/// [`TryFrom<&str>`] by this module's round-trip test.
			#[allow(dead_code)] // Called by VPLDecode-generated metadata; the lint can't see through the proc-macro.
			#[must_use]
			pub fn variants() -> &'static [&'static str] {
				&[$($canonical),+]
			}
		}

		impl TryFrom<&str> for $name {
			type Error = anyhow::Error;

			fn try_from(value: &str) -> Result<Self> {
				Ok(match value.to_lowercase().trim() {
					$($canonical $(| $alias)* => $name::$variant,)+
					other => bail!(
						"Invalid tile format '{other}'; expected one of {}",
						$name::variants().join(", ")
					),
				})
			}
		}

		impl TryFrom<TileFormat> for $name {
			type Error = anyhow::Error;

			fn try_from(value: TileFormat) -> Result<Self> {
				Ok(match value {
					$(TileFormat::$format => $name::$variant,)+
					other => bail!(
						"Invalid tile format '{other}'; expected one of {}",
						$name::variants().join(", ")
					),
				})
			}
		}

		impl From<$name> for TileFormat {
			fn from(value: $name) -> Self {
				match value {
					$($name::$variant => TileFormat::$format),+
				}
			}
		}
	};
}

tile_format_subset!(
	/// The image formats a raster operation can encode to.
	RasterTileFormat {
		Avif => "avif" => AVIF,
		Jpeg => "jpg" | "jpeg" => JPG,
		Png => "png" => PNG,
		Webp => "webp" => WEBP,
	}
);

tile_format_subset!(
	/// What `from_debug` can draw: its vector tile, or one of the raster formats
	/// it renders the coordinate label into.
	DebugTileFormat {
		Mvt => "mvt" | "pbf" => MVT,
		Avif => "avif" => AVIF,
		Jpeg => "jpg" | "jpeg" => JPG,
		Png => "png" => PNG,
		Webp => "webp" => WEBP,
	}
);

#[cfg(test)]
mod tests {
	use super::*;

	/// Guards the promise `variants()` makes to the generated reference: every
	/// listed spelling parses, and it round-trips through `TileFormat`.
	fn assert_round_trip<T>(expected: &'static [&'static str])
	where
		T: TryFrom<&'static str, Error = anyhow::Error> + TryFrom<TileFormat, Error = anyhow::Error> + Copy,
		TileFormat: From<T>,
	{
		for variant in expected {
			let parsed = T::try_from(*variant).unwrap_or_else(|_| panic!("variant {variant:?} does not parse"));
			let format = TileFormat::from(parsed);
			assert!(
				T::try_from(format).is_ok(),
				"variant {variant:?} does not round-trip through TileFormat"
			);
		}
	}

	#[test]
	fn raster_variants_match_try_from() {
		assert_eq!(RasterTileFormat::variants(), &["avif", "jpg", "png", "webp"]);
		assert_round_trip::<RasterTileFormat>(RasterTileFormat::variants());
	}

	#[test]
	fn debug_variants_match_try_from() {
		assert_eq!(DebugTileFormat::variants(), &["mvt", "avif", "jpg", "png", "webp"]);
		assert_round_trip::<DebugTileFormat>(DebugTileFormat::variants());
	}

	#[test]
	fn aliases_parse_but_are_not_listed() {
		assert_eq!(RasterTileFormat::try_from("jpeg").unwrap(), RasterTileFormat::Jpeg);
		assert_eq!(DebugTileFormat::try_from("pbf").unwrap(), DebugTileFormat::Mvt);
		assert!(!RasterTileFormat::variants().contains(&"jpeg"));
		assert!(!DebugTileFormat::variants().contains(&"pbf"));
	}

	#[test]
	fn formats_outside_the_subset_are_rejected() {
		assert!(RasterTileFormat::try_from("geojson").is_err());
		assert!(RasterTileFormat::try_from("mvt").is_err());
		assert!(DebugTileFormat::try_from("geojson").is_err());
		assert!(RasterTileFormat::try_from(TileFormat::GEOJSON).is_err());
		assert!(DebugTileFormat::try_from(TileFormat::SVG).is_err());
	}

	#[test]
	fn raster_parses_every_spelling() {
		use RasterTileFormat::{Avif, Jpeg, Png, Webp};
		for (text, expected) in [
			("avif", Avif),
			("jpg", Jpeg),
			("jpeg", Jpeg),
			("png", Png),
			("webp", Webp),
		] {
			assert_eq!(RasterTileFormat::try_from(text).unwrap(), expected, "parsing {text:?}");
		}
		assert!(RasterTileFormat::try_from("tiff").is_err());
	}

	#[test]
	fn raster_converts_from_tile_format() {
		use RasterTileFormat::{Avif, Jpeg, Png, Webp};
		for (input, expected) in [
			(TileFormat::AVIF, Avif),
			(TileFormat::JPG, Jpeg),
			(TileFormat::PNG, Png),
			(TileFormat::WEBP, Webp),
		] {
			assert_eq!(
				RasterTileFormat::try_from(input).unwrap(),
				expected,
				"converting {input}"
			);
		}
	}

	#[test]
	fn error_message_lists_the_accepted_values() {
		let err = RasterTileFormat::try_from("nope").unwrap_err().to_string();
		assert!(err.contains("avif, jpg, png, webp"), "got: {err}");
	}
}
