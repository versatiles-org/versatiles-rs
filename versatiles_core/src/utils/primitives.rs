use anyhow::{Result, anyhow, ensure};
use num_traits::{Bounded, Float, NumCast, PrimInt};
use std::any::type_name;

pub fn float_to_int<F, I>(value: F) -> Result<I>
where
	F: Float,
	I: PrimInt + Bounded,
{
	ensure!(value.is_finite(), "Value must be finite");

	let n = value.round();

	// Convert integer bounds into the float type for comparison.
	// This should always succeed for normal float+int combos, but we guard anyway.
	let min_f: F = NumCast::from(I::min_value())
		.ok_or_else(|| anyhow!("Cannot represent {}::MIN in float type", type_name::<I>()))?;
	let max_f: F = NumCast::from(I::max_value())
		.ok_or_else(|| anyhow!("Cannot represent {}::MAX in float type", type_name::<I>()))?;

	ensure!(n >= min_f && n <= max_f, "Number out of range for {}", type_name::<I>());

	// Now cast the rounded value to the integer type. If something weird happens, error.
	NumCast::from(n).ok_or_else(|| anyhow!("Failed converting rounded value to {}", type_name::<I>()))
}

#[cfg(test)]
mod tests {
	use super::*;
	use rstest::rstest;

	#[rstest]
	#[case::zero(0.0_f64, 0_i32)]
	#[case::positive(42.0_f64, 42_i32)]
	#[case::negative(-7.0_f64, -7_i32)]
	#[case::rounds_up(2.7_f64, 3_i32)]
	#[case::rounds_down(2.3_f64, 2_i32)]
	#[case::rounds_half_away_from_zero(0.5_f64, 1_i32)]
	#[case::negative_rounds_away_from_zero(-0.5_f64, -1_i32)]
	fn float_to_int_success(#[case] input: f64, #[case] expected: i32) {
		assert_eq!(float_to_int::<f64, i32>(input).unwrap(), expected);
	}

	#[rstest]
	#[case::nan(f64::NAN)]
	#[case::pos_inf(f64::INFINITY)]
	#[case::neg_inf(f64::NEG_INFINITY)]
	fn float_to_int_rejects_non_finite(#[case] value: f64) {
		let err = float_to_int::<f64, i32>(value).unwrap_err();
		assert!(err.to_string().contains("finite"));
	}

	#[rstest]
	#[case::above_max(1.0e12_f64)]
	#[case::below_min(-1.0e12_f64)]
	fn float_to_int_rejects_out_of_range(#[case] value: f64) {
		let err = float_to_int::<f64, i32>(value).unwrap_err();
		assert!(err.to_string().contains("out of range"));
	}

	#[test]
	fn float_to_int_works_for_unsigned() {
		assert_eq!(float_to_int::<f64, u16>(300.0).unwrap(), 300);
		assert!(float_to_int::<f64, u16>(-1.0).is_err());
		assert!(float_to_int::<f64, u16>(70_000.0).is_err());
	}

	#[test]
	fn float_to_int_works_for_f32() {
		assert_eq!(float_to_int::<f32, u8>(128.0_f32).unwrap(), 128);
		assert!(float_to_int::<f32, u8>(300.0_f32).is_err());
	}
}
