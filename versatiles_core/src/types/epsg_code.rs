//! An EPSG code, as a VPL argument writes one.

use std::fmt::{Debug, Display};

use anyhow::{Result, anyhow, bail};

use super::bounds::Bounds;

/// The lowest code the EPSG register assigns.
///
/// From the GeoTIFF convention the register follows: `1024`–`32766` are
/// assigned codes, `32767` means "undefined", and everything above is
/// user-defined and outside the register.
pub const MIN_EPSG_CODE: u32 = 1024;

/// The highest code the EPSG register assigns. See [`MIN_EPSG_CODE`].
pub const MAX_EPSG_CODE: u32 = 32766;

/// A coordinate reference system named by its EPSG code.
///
/// Exists so that a `crs=` or `epsg=` argument carries what it is in its type.
/// `check.rs` names this parameter as the example of what a `u32` cannot say:
/// *"`epsg=99999` is a perfectly good `u32` and not an EPSG code"*. It is now a
/// bad [`EpsgCode`], which `check` reports without building anything (#260).
///
/// # This is shallower than the projection library, on purpose
///
/// The real authority is PROJ's database, and consulting it means reading
/// `proj.db` — I/O, which `check` promises never to do, and which is only
/// present at all in a build with the `gdal` feature. So this checks the shape
/// of a code, not its existence: `4326` and `9999` both pass here, and only the
/// second fails later when GDAL is asked for it. Catching the typo that is off
/// by four digits without pretending to know the register is the trade.
///
/// # `900913` is refused
///
/// GDAL resolves `EPSG:900913` — the legacy Google/OSGeo spelling of web
/// mercator — even though the register has no such code and `projinfo` does not
/// list one. Accepting it here would mean this type describes a set no interval
/// can express, so it is refused with an error naming `3857`, which is the same
/// projection and a real code.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct EpsgCode(u32);

impl EpsgCode {
	/// Builds a code, checking it against the register's range.
	///
	/// # Errors
	///
	/// Errors outside [`MIN_EPSG_CODE`]`..=`[`MAX_EPSG_CODE`].
	pub fn new(code: u32) -> Result<Self> {
		if code == 900_913 {
			bail!("900913 is not an EPSG code, it is a legacy spelling of web mercator — use 3857");
		}
		if !(MIN_EPSG_CODE..=MAX_EPSG_CODE).contains(&code) {
			bail!("EPSG codes run from {MIN_EPSG_CODE} to {MAX_EPSG_CODE}, so {code} is not one");
		}
		Ok(Self(code))
	}

	/// The code as the projection libraries take it.
	#[must_use]
	pub fn code(self) -> u32 {
		self.0
	}

	/// What a form should offer: whole numbers across the register's range.
	///
	/// Replaces the only thing a consumer could previously read off a `u32`
	/// field — `0` to `4294967295`, which is true of the Rust type and useless
	/// as a control (#260).
	#[must_use]
	pub fn bounds() -> Bounds {
		Bounds::integers(f64::from(MIN_EPSG_CODE), f64::from(MAX_EPSG_CODE))
	}
}

impl From<EpsgCode> for u32 {
	fn from(code: EpsgCode) -> Self {
		code.0
	}
}

impl TryFrom<&str> for EpsgCode {
	type Error = anyhow::Error;

	/// Parses the code as VPL writes it.
	fn try_from(value: &str) -> Result<Self> {
		let text = value.trim();
		let code = text
			.parse::<u32>()
			.map_err(|_| anyhow!("EPSG code '{text}' is not a whole number"))?;
		Self::new(code)
	}
}

impl Display for EpsgCode {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		Display::fmt(&self.0, f)
	}
}

impl Debug for EpsgCode {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "EPSG:{}", self.0)
	}
}

#[cfg(test)]
mod tests {
	use rstest::rstest;

	use super::*;

	/// Codes a projection library resolves, sampled across the register.
	#[rstest]
	#[case(2000)]
	#[case(3035)]
	#[case(3068)]
	#[case(3785)]
	#[case(3857)]
	#[case(4326)]
	#[case(25832)]
	#[case(32766)]
	fn a_real_code_is_accepted(#[case] code: u32) {
		assert!(EpsgCode::new(code).is_ok(), "EPSG:{code} should be accepted");
	}

	#[rstest]
	#[case::zero(0, "run from 1024 to 32766")]
	#[case::below_the_register(999, "run from 1024 to 32766")]
	#[case::undefined_marker(32767, "run from 1024 to 32766")]
	#[case::a_typo(99999, "run from 1024 to 32766")]
	#[case::an_esri_code(102113, "run from 1024 to 32766")]
	#[case::the_whole_u32(u32::MAX, "run from 1024 to 32766")]
	#[case::legacy_mercator(900_913, "use 3857")]
	fn a_code_outside_the_register_says_why(#[case] code: u32, #[case] expected: &str) {
		let error = EpsgCode::new(code).expect_err("should be rejected");
		assert!(error.to_string().contains(expected), "got: {error}");
	}

	#[test]
	fn parsing_reports_the_two_mistakes_separately() {
		assert_eq!(EpsgCode::try_from("4326").unwrap().code(), 4326);
		assert_eq!(EpsgCode::try_from(" 4326 ").unwrap().code(), 4326);

		let error = EpsgCode::try_from("wgs84").unwrap_err();
		assert!(error.to_string().contains("is not a whole number"), "{error}");
	}

	/// The range a consumer renders is the range the parser enforces.
	#[test]
	fn the_advertised_bounds_are_the_ones_the_parser_enforces() {
		let bounds = EpsgCode::bounds();
		assert_eq!((bounds.min, bounds.max), (Some(1024.0), Some(32766.0)));
		assert!(bounds.integer);

		assert!(EpsgCode::new(MIN_EPSG_CODE).is_ok() && EpsgCode::new(MAX_EPSG_CODE).is_ok());
		assert!(EpsgCode::new(MIN_EPSG_CODE - 1).is_err());
		assert!(EpsgCode::new(MAX_EPSG_CODE + 1).is_err());
	}

	#[test]
	fn a_code_renders_as_a_code() {
		assert_eq!(EpsgCode::new(4326).unwrap().to_string(), "4326");
		assert_eq!(format!("{:?}", EpsgCode::new(4326).unwrap()), "EPSG:4326");
	}
}
