//! The process's use of CPU, memory and disk, as the operating system counts it.
//!
//! [`ResourceUsage::read`] takes one reading, and [`ResourceUsage::since`]
//! turns two readings into rates. What a platform cannot tell is `None`: Linux
//! gives everything, macOS everything but the machine's I/O wait, other
//! platforms nothing.

use std::time::{Duration, Instant};

/// One reading of the process's resource counters.
///
/// The counters run from the start of the process, so one reading says little
/// on its own: compare two with [`since`](Self::since).
#[derive(Clone, Copy, Debug)]
pub struct ResourceUsage {
	/// When the reading was taken.
	pub at: Instant,
	/// CPU time of all threads, in user and kernel mode together.
	pub cpu_time: Option<Duration>,
	/// Memory resident in RAM.
	pub rss_bytes: Option<u64>,
	/// The most memory resident in RAM so far.
	pub peak_rss_bytes: Option<u64>,
	/// Bytes read from storage. Reads served from the page cache don't count.
	pub disk_read_bytes: Option<u64>,
	/// Bytes written to storage.
	pub disk_written_bytes: Option<u64>,
	/// The whole machine's CPU time, for the share spent waiting on I/O.
	pub machine_cpu: Option<MachineCpu>,
}

/// The whole machine's CPU time counters, in clock ticks, as Linux keeps them
/// in `/proc/stat`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MachineCpu {
	/// All CPU time, busy or not.
	pub total: u64,
	/// Time a CPU was idle while I/O was outstanding.
	pub iowait: u64,
}

/// What happened between two readings, as rates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UsageRates {
	/// Seconds between the two readings.
	pub seconds: f64,
	/// Share of the available cores busy with this process, from 0 to 1.
	pub cpu: Option<f64>,
	/// Share of the machine's CPU time spent waiting on I/O, from 0 to 1. It
	/// covers the whole machine, so other processes count too.
	pub iowait: Option<f64>,
	/// Bytes read from storage per second.
	pub disk_read_per_second: Option<f64>,
	/// Bytes written to storage per second.
	pub disk_written_per_second: Option<f64>,
}

/// Cores this process may run on. Respects CPU affinity and, on Linux,
/// cgroup quotas, so a container limited to 8 of 32 cores gets 8.
#[must_use]
pub fn available_cores() -> usize {
	std::thread::available_parallelism().map_or(1, std::num::NonZero::get)
}

impl ResourceUsage {
	/// Reads the counters now.
	#[must_use]
	pub fn read() -> Self {
		#[cfg_attr(not(unix), expect(unused_mut, reason = "only the Unix readers below fill it in"))]
		let mut usage = Self {
			at: Instant::now(),
			cpu_time: None,
			rss_bytes: None,
			peak_rss_bytes: None,
			disk_read_bytes: None,
			disk_written_bytes: None,
			machine_cpu: None,
		};
		#[cfg(unix)]
		read_rusage(&mut usage);
		#[cfg(target_os = "linux")]
		read_proc(&mut usage);
		#[cfg(target_os = "macos")]
		read_proc_pid_rusage(&mut usage);
		usage
	}

	/// The rates between `earlier` and this reading, with CPU time shared out
	/// over `cores`.
	#[must_use]
	pub fn since(&self, earlier: &Self, cores: usize) -> UsageRates {
		let seconds = self.at.saturating_duration_since(earlier.at).as_secs_f64();
		let per_second = |now: Option<u64>, then: Option<u64>| -> Option<f64> {
			(seconds > 0.0).then_some(now?.saturating_sub(then?) as f64 / seconds)
		};
		let cpu = match (self.cpu_time, earlier.cpu_time) {
			(Some(now), Some(then)) if seconds > 0.0 => {
				Some(now.saturating_sub(then).as_secs_f64() / seconds / cores.max(1) as f64)
			}
			_ => None,
		};
		let iowait = match (self.machine_cpu, earlier.machine_cpu) {
			(Some(now), Some(then)) if now.total > then.total => {
				Some(now.iowait.saturating_sub(then.iowait) as f64 / (now.total - then.total) as f64)
			}
			_ => None,
		};
		UsageRates {
			seconds,
			cpu,
			iowait,
			disk_read_per_second: per_second(self.disk_read_bytes, earlier.disk_read_bytes),
			disk_written_per_second: per_second(self.disk_written_bytes, earlier.disk_written_bytes),
		}
	}
}

/// CPU time and peak memory, from `getrusage`.
#[cfg(unix)]
fn read_rusage(usage: &mut ResourceUsage) {
	let mut raw = std::mem::MaybeUninit::<libc::rusage>::zeroed();
	#[expect(
		unsafe_code,
		reason = "getrusage fills in the struct it is given; see the SAFETY note"
	)]
	// SAFETY: `raw` is a writable `rusage`. Zeroed memory is a valid `rusage`,
	// which holds only integers, so it may be read whether or not the call
	// wrote to it.
	let (result, raw) = unsafe { (libc::getrusage(libc::RUSAGE_SELF, raw.as_mut_ptr()), raw.assume_init()) };
	if result != 0 {
		return;
	}
	let time = |t: libc::timeval| {
		Duration::from_secs(u64::try_from(t.tv_sec).unwrap_or(0))
			+ Duration::from_micros(u64::try_from(t.tv_usec).unwrap_or(0))
	};
	usage.cpu_time = Some(time(raw.ru_utime) + time(raw.ru_stime));
	// Bytes on macOS, KiB everywhere else.
	let peak = u64::try_from(raw.ru_maxrss).unwrap_or(0);
	usage.peak_rss_bytes = Some(if cfg!(target_os = "macos") {
		peak
	} else {
		peak.saturating_mul(1024)
	});
}

/// Resident memory, disk I/O and the machine's CPU time, from `/proc`.
#[cfg(target_os = "linux")]
fn read_proc(usage: &mut ResourceUsage) {
	let read = |path: &str| std::fs::read_to_string(path).ok();
	usage.rss_bytes = read("/proc/self/status").as_deref().and_then(parse_status_rss);
	if let Some(io) = read("/proc/self/io") {
		(usage.disk_read_bytes, usage.disk_written_bytes) = parse_io(&io);
	}
	usage.machine_cpu = read("/proc/stat").as_deref().and_then(parse_stat);
}

/// Resident memory and disk I/O, from `proc_pid_rusage`.
#[cfg(target_os = "macos")]
fn read_proc_pid_rusage(usage: &mut ResourceUsage) {
	let mut info = std::mem::MaybeUninit::<libc::rusage_info_v2>::zeroed();
	#[expect(
		unsafe_code,
		reason = "proc_pid_rusage fills in the struct it is given; see the SAFETY note"
	)]
	// SAFETY: for `RUSAGE_INFO_V2` the call writes at most one
	// `rusage_info_v2` to the buffer, and the buffer is one. Zeroed memory is
	// a valid `rusage_info_v2`, which holds only integers, so it may be read
	// whether or not the call wrote to it.
	let (result, info) = unsafe {
		let result = libc::proc_pid_rusage(libc::getpid(), libc::RUSAGE_INFO_V2, info.as_mut_ptr().cast());
		(result, info.assume_init())
	};
	if result != 0 {
		return;
	}
	usage.rss_bytes = Some(info.ri_resident_size);
	usage.disk_read_bytes = Some(info.ri_diskio_bytesread);
	usage.disk_written_bytes = Some(info.ri_diskio_byteswritten);
}

/// `VmRSS` from `/proc/self/status`, in bytes.
#[cfg(any(target_os = "linux", test))]
fn parse_status_rss(status: &str) -> Option<u64> {
	let line = status.lines().find_map(|line| line.strip_prefix("VmRSS:"))?;
	let kib: u64 = line.split_whitespace().next()?.parse().ok()?;
	Some(kib.saturating_mul(1024))
}

/// `read_bytes` and `write_bytes` from `/proc/self/io`.
#[cfg(any(target_os = "linux", test))]
fn parse_io(io: &str) -> (Option<u64>, Option<u64>) {
	let field = |name: &str| {
		io.lines()
			.find_map(|line| line.strip_prefix(name)?.strip_prefix(':'))
			.and_then(|value| value.trim().parse().ok())
	};
	(field("read_bytes"), field("write_bytes"))
}

/// The machine's CPU time from the first line of `/proc/stat`: `cpu user nice
/// system idle iowait irq softirq steal guest guest_nice`. Guest time is
/// already part of user time, so it is left out of the total.
#[cfg(any(target_os = "linux", test))]
fn parse_stat(stat: &str) -> Option<MachineCpu> {
	let line = stat.lines().next()?.strip_prefix("cpu ")?;
	let ticks: Vec<u64> = line
		.split_whitespace()
		.take(8)
		.map(str::parse)
		.collect::<Result<_, _>>()
		.ok()?;
	Some(MachineCpu {
		total: ticks.iter().sum(),
		iowait: *ticks.get(4)?,
	})
}

#[cfg(test)]
mod tests {
	use super::*;

	fn reading(at: Instant, cpu_seconds: f64, read: u64, cpu: MachineCpu) -> ResourceUsage {
		ResourceUsage {
			at,
			cpu_time: Some(Duration::from_secs_f64(cpu_seconds)),
			rss_bytes: Some(1),
			peak_rss_bytes: Some(1),
			disk_read_bytes: Some(read),
			disk_written_bytes: None,
			machine_cpu: Some(cpu),
		}
	}

	#[test]
	fn rates_share_cpu_time_over_the_cores() {
		let start = Instant::now();
		let earlier = reading(start, 10.0, 1000, MachineCpu { total: 100, iowait: 10 });
		let later = reading(
			start + Duration::from_secs(2),
			18.0,
			5000,
			MachineCpu { total: 300, iowait: 60 },
		);
		let rates = later.since(&earlier, 8);
		assert_eq!(rates.seconds, 2.0);
		assert_eq!(rates.cpu, Some(0.5), "8 CPU seconds in 2 s on 8 cores");
		assert_eq!(rates.iowait, Some(0.25), "50 of 200 ticks");
		assert_eq!(rates.disk_read_per_second, Some(2000.0));
		assert_eq!(rates.disk_written_per_second, None, "unknown stays unknown");
	}

	#[test]
	fn readings_at_the_same_moment_have_no_rates() {
		let at = Instant::now();
		let one = reading(at, 1.0, 1, MachineCpu { total: 1, iowait: 0 });
		let rates = one.since(&one, 4);
		assert_eq!(rates.cpu, None);
		assert_eq!(rates.iowait, None);
		assert_eq!(rates.disk_read_per_second, None);
	}

	#[test]
	fn proc_files_are_parsed() {
		let status = "Name:\tversatiles\nVmPeak:\t  999 kB\nVmRSS:\t  2048 kB\nThreads:\t4\n";
		assert_eq!(parse_status_rss(status), Some(2048 * 1024));

		let io =
			"rchar: 1\nwchar: 2\nsyscr: 3\nsyscw: 4\nread_bytes: 4096\nwrite_bytes: 8192\ncancelled_write_bytes: 0\n";
		assert_eq!(parse_io(io), (Some(4096), Some(8192)));

		let stat = "cpu  10 1 5 80 3 0 1 0 7 0\ncpu0 5 0 2 40 1 0 0 0 0 0\n";
		assert_eq!(parse_stat(stat), Some(MachineCpu { total: 100, iowait: 3 }));
		assert_eq!(parse_stat("intr 1 2 3\n"), None);
	}

	#[cfg(unix)]
	#[test]
	fn cpu_time_grows_while_busy() {
		let before = ResourceUsage::read();
		let mut x = 0u64;
		let started = Instant::now();
		while started.elapsed() < Duration::from_millis(50) {
			x = std::hint::black_box(x.wrapping_mul(31).wrapping_add(7));
		}
		let after = ResourceUsage::read();
		assert!(after.cpu_time > before.cpu_time, "{before:?} → {after:?}");
		assert!(after.peak_rss_bytes.is_some_and(|peak| peak > 0));
	}

	#[cfg(any(target_os = "linux", target_os = "macos"))]
	#[test]
	fn resident_memory_and_disk_counters_are_read() {
		let usage = ResourceUsage::read();
		assert!(usage.rss_bytes.is_some_and(|rss| rss > 0));
		assert!(usage.disk_read_bytes.is_some());
	}
}
