/// A byte count for log messages and diagnostics, in the largest binary unit
/// below 1024 of it, with one decimal: `1536` is `"1.5 KiB"`.
#[must_use]
pub fn format_bytes(n: u64) -> String {
	const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
	let mut value = n as f64;
	let mut unit = UNITS[0];
	for next in &UNITS[1..] {
		if value < 1024.0 {
			break;
		}
		value /= 1024.0;
		unit = next;
	}
	format!("{value:.1} {unit}")
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn format_bytes_picks_the_largest_unit_below_1024() {
		assert_eq!(format_bytes(0), "0.0 B");
		assert_eq!(format_bytes(1023), "1023.0 B");
		assert_eq!(format_bytes(1024), "1.0 KiB");
		assert_eq!(format_bytes(1536), "1.5 KiB");
		assert_eq!(format_bytes(3 * 1024 * 1024 / 2), "1.5 MiB");
		assert_eq!(format_bytes(3 * 1024 * 1024 * 1024), "3.0 GiB");
		assert_eq!(format_bytes(1 << 40), "1.0 TiB");
		assert_eq!(format_bytes(1 << 50), "1024.0 TiB");
		assert_eq!(format_bytes(u64::MAX), "16777216.0 TiB");
	}
}
