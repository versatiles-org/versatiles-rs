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
