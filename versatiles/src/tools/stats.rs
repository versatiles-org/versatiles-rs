//! `--stats` and `--stats-file`: how much CPU, memory and disk a command uses
//! while it runs.
//!
//! The readings come from [`ResourceUsage`]. A task takes one each second,
//! writes it to the stats file, logs a line every ten seconds and a summary at
//! the end.

use std::{
	fs::File,
	io::{BufWriter, Write},
	path::PathBuf,
	time::Duration,
};

use anyhow::{Context, Result, anyhow};
use tokio::{sync::oneshot, task::JoinHandle};
use versatiles_container::{ResourceUsage, UsageRates, available_cores};
use versatiles_core::utils::format_bytes;

/// How often a reading is taken.
const SAMPLE_EVERY: Duration = Duration::from_secs(1);
/// How many readings go into each logged line.
const SAMPLES_PER_REPORT: u32 = 10;

/// The options that turn the monitor on.
#[derive(clap::Args, Debug, Default)]
pub struct StatsOptions {
	/// report CPU, memory and disk usage every 10 seconds, and in total at the end
	///
	/// CPU is the share of the cores this process may use. A low share with a
	/// high iowait (Linux only) means the cores wait on storage, not on the
	/// pipeline.
	#[arg(long, display_order = 5, verbatim_doc_comment)]
	stats: bool,

	/// write CPU, memory and disk usage to this file once a second, as JSON Lines
	///
	/// Each line holds the share of the cores busy and of I/O wait in the last
	/// second, and the CPU time, resident memory and bytes read and written
	/// since the start. Each line is written straight away, so the file is
	/// complete up to the last second even if the process is killed.
	#[arg(long, value_name = "file", display_order = 5, verbatim_doc_comment)]
	stats_file: Option<PathBuf>,
}

impl StatsOptions {
	/// Starts the monitor, or returns `None` when neither option is given.
	///
	/// Must be called from within a Tokio runtime.
	pub fn start(&self) -> Result<Option<Monitor>> {
		if !self.stats && self.stats_file.is_none() {
			return Ok(None);
		}
		let file = match &self.stats_file {
			Some(path) => Some(BufWriter::new(
				File::create(path).with_context(|| format!("creating the stats file {}", path.display()))?,
			)),
			None => None,
		};
		Ok(Some(Monitor::start(self.stats, file, SAMPLE_EVERY, SAMPLES_PER_REPORT)))
	}
}

/// The running monitor. [`finish`](Self::finish) stops it and logs the summary;
/// dropping it stops it silently.
pub struct Monitor {
	stop: Option<oneshot::Sender<()>>,
	task: JoinHandle<Result<String>>,
	report: bool,
}

impl Monitor {
	fn start(report: bool, file: Option<BufWriter<File>>, every: Duration, per_report: u32) -> Self {
		let (stop, stopped) = oneshot::channel();
		let task = tokio::spawn(sample(report, file, every, per_report, stopped));
		Self {
			stop: Some(stop),
			task,
			report,
		}
	}

	/// Takes a last reading and, with `--stats`, logs the totals.
	pub async fn finish(mut self) -> Result<()> {
		if let Some(stop) = self.stop.take() {
			// The task only ends after this, so the receiver is still there.
			let _ = stop.send(());
		}
		let summary = (&mut self.task)
			.await
			.map_err(|error| anyhow!("the stats task failed: {error}"))??;
		if self.report {
			log::info!("stats: {summary}");
		}
		Ok(())
	}
}

impl Drop for Monitor {
	fn drop(&mut self) {
		self.task.abort();
	}
}

/// Takes a reading every `every` until `stopped` fires, and returns the summary.
async fn sample(
	report: bool,
	mut file: Option<BufWriter<File>>,
	every: Duration,
	per_report: u32,
	mut stopped: oneshot::Receiver<()>,
) -> Result<String> {
	let cores = available_cores();
	let first = ResourceUsage::read();
	let mut last = first;
	let mut last_report = first;
	let mut shares = Vec::new();
	let mut peak_rss = 0;
	let mut ticker = tokio::time::interval(every);
	ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
	ticker.tick().await;
	let mut count = 0u32;
	loop {
		let stopping = tokio::select! {
			_ = ticker.tick() => false,
			_ = &mut stopped => true,
		};
		let now = ResourceUsage::read();
		let rates = now.since(&last, cores);
		peak_rss = peak_rss.max(now.rss_bytes.unwrap_or(0));
		// The last interval is cut short, and a share over a few milliseconds
		// would skew the median.
		if !stopping && let Some(cpu) = rates.cpu {
			shares.push(cpu);
		}
		if let Some(file) = &mut file {
			writeln!(file, "{}", json_line(&now, &first, &rates, cores)).context("writing the stats file")?;
			file.flush().context("writing the stats file")?;
		}
		count += 1;
		if report && !stopping && count.is_multiple_of(per_report) {
			log::info!("stats: {}", report_line(&now.since(&last_report, cores), &now, cores));
			last_report = now;
		}
		last = now;
		if stopping {
			let peak = now.peak_rss_bytes.unwrap_or(0).max(peak_rss);
			return Ok(summary_line(
				&now.since(&first, cores),
				&now,
				&first,
				&mut shares,
				peak,
				cores,
			));
		}
	}
}

/// One logged line: the rates since the last one, and memory now.
fn report_line(rates: &UsageRates, now: &ResourceUsage, cores: usize) -> String {
	let mut parts = Vec::new();
	if let Some(cpu) = rates.cpu {
		parts.push(format!("CPU {} of {cores} cores", percent(cpu)));
	}
	if let Some(iowait) = rates.iowait {
		parts.push(format!("iowait {}", percent(iowait)));
	}
	if let Some(rss) = now.rss_bytes {
		let peak = now
			.peak_rss_bytes
			.map_or(String::new(), |peak| format!(" (peak {})", format_bytes(peak.max(rss))));
		parts.push(format!("RSS {}{peak}", format_bytes(rss)));
	}
	if let (Some(read), Some(written)) = (rates.disk_read_per_second, rates.disk_written_per_second) {
		parts.push(format!("disk read {}/s, written {}/s", bytes(read), bytes(written)));
	}
	if parts.is_empty() {
		return "not available on this platform".to_string();
	}
	parts.join(", ")
}

/// The totals over the whole run.
fn summary_line(
	rates: &UsageRates,
	now: &ResourceUsage,
	first: &ResourceUsage,
	shares: &mut [f64],
	peak_rss: u64,
	cores: usize,
) -> String {
	let mut parts = vec![format!("{} in total", duration(rates.seconds))];
	if let Some(cpu) = rates.cpu {
		let median = median(shares).map_or(String::new(), |median| format!(", median {}", percent(median)));
		parts.push(format!("CPU mean {}{median} of {cores} cores", percent(cpu)));
	}
	if let Some(iowait) = rates.iowait {
		parts.push(format!("iowait mean {}", percent(iowait)));
	}
	if peak_rss > 0 {
		parts.push(format!("peak RSS {}", format_bytes(peak_rss)));
	}
	if let (Some(read), Some(written), Some(read_rate), Some(written_rate)) = (
		difference(now.disk_read_bytes, first.disk_read_bytes),
		difference(now.disk_written_bytes, first.disk_written_bytes),
		rates.disk_read_per_second,
		rates.disk_written_per_second,
	) {
		parts.push(format!(
			"disk read {} ({}/s), written {} ({}/s)",
			format_bytes(read),
			bytes(read_rate),
			format_bytes(written),
			bytes(written_rate)
		));
	}
	parts.join(", ")
}

/// One line of the stats file. Counters run from the start of the monitor;
/// `cpu` and `iowait` cover the interval since the line before.
fn json_line(now: &ResourceUsage, first: &ResourceUsage, rates: &UsageRates, cores: usize) -> String {
	let seconds = now.at.saturating_duration_since(first.at).as_secs_f64();
	let cpu_seconds = match (now.cpu_time, first.cpu_time) {
		(Some(now), Some(first)) => Some(now.saturating_sub(first).as_secs_f64()),
		_ => None,
	};
	let fields = [
		("seconds", fraction(Some(seconds))),
		("cores", cores.to_string()),
		("cpu", fraction(rates.cpu)),
		("iowait", fraction(rates.iowait)),
		("cpu_seconds", fraction(cpu_seconds)),
		("rss_bytes", integer(now.rss_bytes)),
		("peak_rss_bytes", integer(now.peak_rss_bytes)),
		(
			"disk_read_bytes",
			integer(difference(now.disk_read_bytes, first.disk_read_bytes)),
		),
		(
			"disk_written_bytes",
			integer(difference(now.disk_written_bytes, first.disk_written_bytes)),
		),
	];
	let body: Vec<String> = fields.iter().map(|(key, value)| format!("\"{key}\":{value}")).collect();
	format!("{{{}}}", body.join(","))
}

fn difference(now: Option<u64>, first: Option<u64>) -> Option<u64> {
	Some(now?.saturating_sub(first?))
}

fn fraction(value: Option<f64>) -> String {
	match value {
		Some(value) if value.is_finite() => format!("{value:.3}"),
		_ => "null".to_string(),
	}
}

fn integer(value: Option<u64>) -> String {
	value.map_or("null".to_string(), |value| value.to_string())
}

fn percent(share: f64) -> String {
	format!("{:.0} %", share * 100.0)
}

#[expect(
	clippy::cast_possible_truncation,
	clippy::cast_sign_loss,
	reason = "a non-negative byte rate, shown rounded"
)]
fn bytes(per_second: f64) -> String {
	format_bytes(per_second.max(0.0).round() as u64)
}

#[expect(
	clippy::cast_possible_truncation,
	clippy::cast_sign_loss,
	reason = "a non-negative number of seconds, shown rounded"
)]
fn duration(seconds: f64) -> String {
	if seconds < 60.0 {
		return format!("{seconds:.1} s");
	}
	let seconds = seconds.round() as u64;
	let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
	if hours > 0 {
		format!("{hours} h {minutes:02} min")
	} else {
		format!("{minutes} min {seconds:02} s")
	}
}

fn median(values: &mut [f64]) -> Option<f64> {
	if values.is_empty() {
		return None;
	}
	values.sort_by(f64::total_cmp);
	let middle = values.len() / 2;
	Some(if values.len().is_multiple_of(2) {
		f64::midpoint(values[middle - 1], values[middle])
	} else {
		values[middle]
	})
}

#[cfg(test)]
mod tests {
	use std::time::Instant;

	use versatiles_container::MachineCpu;
	use versatiles_core::json::JsonObject;

	use super::*;

	fn reading(at: Instant, cpu_seconds: u64, rss: u64, read: u64, written: u64) -> ResourceUsage {
		ResourceUsage {
			at,
			cpu_time: Some(Duration::from_secs(cpu_seconds)),
			rss_bytes: Some(rss),
			peak_rss_bytes: Some(rss),
			disk_read_bytes: Some(read),
			disk_written_bytes: Some(written),
			machine_cpu: Some(MachineCpu {
				total: cpu_seconds * 10,
				iowait: cpu_seconds,
			}),
		}
	}

	const MIB: u64 = 1024 * 1024;

	#[test]
	fn a_report_names_cpu_iowait_memory_and_disk() {
		let start = Instant::now();
		let first = reading(start, 0, 100 * MIB, 0, 0);
		let now = reading(start + Duration::from_secs(10), 40, 200 * MIB, 50 * MIB, 10 * MIB);
		assert_eq!(
			report_line(&now.since(&first, 8), &now, 8),
			"CPU 50 % of 8 cores, iowait 10 %, RSS 200.0 MiB (peak 200.0 MiB), disk read 5.0 MiB/s, written 1.0 MiB/s"
		);
	}

	#[test]
	fn the_summary_gives_mean_and_median() {
		let start = Instant::now();
		let first = reading(start, 0, 100 * MIB, 0, 0);
		let now = reading(
			start + Duration::from_secs(7_500),
			30_000,
			200 * MIB,
			3 * 1024 * MIB,
			MIB,
		);
		let mut shares = vec![0.9, 0.1, 0.2];
		assert_eq!(
			summary_line(&now.since(&first, 8), &now, &first, &mut shares, 300 * MIB, 8),
			"2 h 05 min in total, CPU mean 50 %, median 20 % of 8 cores, iowait mean 10 %, peak RSS 300.0 MiB, \
			 disk read 3.0 GiB (419.4 KiB/s), written 1.0 MiB (140.0 B/s)"
		);
	}

	#[test]
	fn a_json_line_is_valid_json() -> Result<()> {
		let start = Instant::now();
		let first = reading(start, 3, 100, 10, 20);
		let now = reading(start + Duration::from_secs(2), 5, 300, 110, 20);
		let mut rates = now.since(&first, 4);
		rates.iowait = None;
		let line = json_line(&now, &first, &rates, 4);
		assert_eq!(
			line,
			r#"{"seconds":2.000,"cores":4,"cpu":0.250,"iowait":null,"cpu_seconds":2.000,"rss_bytes":300,"peak_rss_bytes":300,"disk_read_bytes":100,"disk_written_bytes":0}"#
		);
		JsonObject::parse_str(&line)?;
		Ok(())
	}

	#[test]
	fn durations_read_naturally() {
		assert_eq!(duration(15.24), "15.2 s");
		assert_eq!(duration(75.0), "1 min 15 s");
		assert_eq!(duration(3.0 * 3600.0 + 61.0), "3 h 01 min");
	}

	#[test]
	fn the_median_of_an_even_count_is_the_midpoint() {
		assert_eq!(median(&mut []), None);
		assert_eq!(median(&mut [3.0, 1.0, 2.0]), Some(2.0));
		assert_eq!(median(&mut [4.0, 1.0, 2.0, 3.0]), Some(2.5));
	}

	#[tokio::test]
	async fn the_file_gets_a_line_per_reading() -> Result<()> {
		let dir = tempfile::tempdir()?;
		let path = dir.path().join("stats.jsonl");
		let monitor = Monitor::start(
			false,
			Some(BufWriter::new(File::create(&path)?)),
			Duration::from_millis(10),
			SAMPLES_PER_REPORT,
		);
		tokio::time::sleep(Duration::from_millis(100)).await;
		monitor.finish().await?;
		let text = std::fs::read_to_string(&path)?;
		let lines: Vec<&str> = text.lines().collect();
		assert!(lines.len() >= 3, "{text}");
		for line in lines {
			JsonObject::parse_str(line)?;
		}
		Ok(())
	}
}
