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
pub fn generate_test_data(size: usize) -> Blob {
	let mut data = Vec::with_capacity(size);
	for i in 0..size {
		data.push((((i as f64 + 1.0).cos() * 1_000_000.0) as u8).wrapping_add(i as u8));
	}
	Blob::from(data)
}
