use crate::Blob;

/// Generates deterministic pseudo-random binary data of a specified size.
///
/// # Arguments
///
/// * `size` - The size of the data to generate.
///
/// # Returns
///
/// * `Blob` containing the generated data.
#[must_use]
pub fn generate_test_data(size: usize) -> Blob {
	let mut data = Vec::with_capacity(size);
	for i in 0..size {
		let v = (i as f64 + 1.0).sin() * 1_000_000.0 + i as f64;
		data.push((v % 256.0) as u8);
	}
	Blob::from(data)
}
