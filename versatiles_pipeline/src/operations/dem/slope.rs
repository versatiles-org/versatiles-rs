//! What MapLibre does to slopes when it shades DEM tiles, for the operations
//! that have to agree with it: `dem_quantize`, which must not add a step a
//! shaded map would show, and `dem_hillshade`, which renders that shading.

/// How much MapLibre multiplies slopes when it hillshades a tile of zoom `level`.
///
/// Mirrors `hillshade_prepare.fragment.glsl`: below zoom 15 the derivative is
/// scaled by `2^((15 - z) * f)`, with `f` 0.4 below zoom 2, 0.35 below zoom 4.5
/// and 0.3 above, so relief stays visible at low zoom. That is 64× at zoom 0,
/// which turns a step that is invisible in the true slope into a terrace.
/// MapLibre passes the overscaled zoom, so a tile shown overzoomed is
/// exaggerated less; its native zoom is the worst case.
pub fn maplibre_slope_exaggeration(level: u8) -> f64 {
	let z = f64::from(level);
	let factor = if z < 2.0 {
		0.4
	} else if z < 4.5 {
		0.35
	} else {
		0.3
	};
	if z < 15.0 { ((15.0 - z) * factor).exp2() } else { 1.0 }
}
