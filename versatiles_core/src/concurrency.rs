//! Concurrency limit tuning for optimal I/O and CPU performance
//!
//! This module provides optimized concurrency limits based on workload type:
//! - **I/O-bound**: Network/disk operations benefit from 2-4x CPU count
//! - **CPU-bound**: Computation limited to 1x CPU count
//! - **Mixed**: Balanced workload at 1.5x CPU count
//!
//! # Usage
//!
//! ```
//! use versatiles_core::ConcurrencyLimits;
//!
//! let limits = ConcurrencyLimits::default();
//!
//! // For I/O-bound operations (network, disk)
//! let io_limit = limits.io_bound;
//!
//! // For CPU-bound operations (image processing, compression)
//! let cpu_limit = limits.cpu_bound;
//!
//! // For mixed workloads
//! let mixed_limit = limits.mixed;
//!
//! assert!(io_limit > cpu_limit);  // I/O should be higher than CPU
//! ```

use num_cpus;

/// Optimized concurrency limits for different workload types
///
/// These limits are based on empirical testing and provide optimal
/// throughput for different operation characteristics.
#[derive(Debug, Clone, Copy)]
pub struct ConcurrencyLimits {
	/// Concurrency for I/O-bound operations (network, disk reads/writes)
	///
	/// Set to 3x CPU count because I/O operations spend most time waiting,
	/// allowing high parallelism without CPU saturation.
	pub io_bound: usize,

	/// Concurrency for CPU-bound operations (image processing, compression)
	///
	/// Set to 1x CPU count to avoid context switching overhead and
	/// ensure optimal CPU utilization without thrashing.
	pub cpu_bound: usize,

	/// Concurrency for mixed workloads (both I/O and CPU)
	///
	/// Set to 1.5x CPU count as a balanced middle ground between
	/// I/O parallelism and CPU efficiency.
	pub mixed: usize,
}

impl ConcurrencyLimits {
	/// Create concurrency limits with custom values
	///
	/// # Arguments
	///
	/// * `io_bound` - Limit for I/O-bound operations
	/// * `cpu_bound` - Limit for CPU-bound operations
	/// * `mixed` - Limit for mixed workloads
	pub fn new(io_bound: usize, cpu_bound: usize, mixed: usize) -> Self {
		Self {
			io_bound: io_bound.max(1),
			cpu_bound: cpu_bound.max(1),
			mixed: mixed.max(1),
		}
	}

	/// Get the number of logical CPUs available
	pub fn cpu_count() -> usize {
		num_cpus::get()
	}
}

impl Default for ConcurrencyLimits {
	/// Create default concurrency limits based on available CPU count
	///
	/// - I/O-bound: 3x CPU count
	/// - CPU-bound: 1x CPU count
	/// - Mixed: 1.5x CPU count
	fn default() -> Self {
		let cpus = num_cpus::get();
		Self {
			io_bound: cpus * 3,
			cpu_bound: cpus,
			mixed: cpus + (cpus / 2),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_default_limits() {
		let limits = ConcurrencyLimits::default();
		let cpus = num_cpus::get();

		assert_eq!(limits.cpu_bound, cpus);
		assert_eq!(limits.io_bound, cpus * 3);
		assert_eq!(limits.mixed, cpus + (cpus / 2));
	}

	#[test]
	fn test_custom_limits() {
		let limits = ConcurrencyLimits::new(12, 4, 8);

		assert_eq!(limits.io_bound, 12);
		assert_eq!(limits.cpu_bound, 4);
		assert_eq!(limits.mixed, 8);
	}

	#[test]
	fn test_limits_minimum_one() {
		let limits = ConcurrencyLimits::new(0, 0, 0);

		assert_eq!(limits.io_bound, 1);
		assert_eq!(limits.cpu_bound, 1);
		assert_eq!(limits.mixed, 1);
	}

	#[test]
	fn test_cpu_count() {
		let count = ConcurrencyLimits::cpu_count();
		assert!(count >= 1);
	}
}
