//! `--stats` and `--stats-file`: how much CPU, memory and disk a command uses
//! while it runs.
//!
//! The readings come from [`ResourceUsage`] for the process, and from the
//! runtime's stages for each operation of a pipeline: the cores its own work
//! keeps busy, and how many requests wait for its tiles. A task takes one
//! reading each second, writes it to the stats file, logs a line every ten
//! seconds and a summary at the end.

use std::{
	fmt::Write as _,
	fs::File,
	io::{BufWriter, Write},
	path::PathBuf,
	time::Duration,
};

use anyhow::{Context, Result, anyhow};
use tokio::{sync::oneshot, task::JoinHandle};
use versatiles_container::{ResourceUsage, TilesRuntime, UsageRates, available_cores};
use versatiles_core::{StageStats, utils::format_bytes};

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
	/// pipeline. For a pipeline, each operation gets the cores its own work
	/// keeps busy, and "waited on": how many requests waited for its tiles on
	/// average. The operation that is waited on much more than it waits on its
	/// input holds the others up.
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
	/// `runtime` measures the stages of what is built under it from now on.
	///
	/// Must be called from within a Tokio runtime.
	pub fn start(&self, runtime: &TilesRuntime) -> Result<Option<Monitor>> {
		if !self.stats && self.stats_file.is_none() {
			return Ok(None);
		}
		let file = match &self.stats_file {
			Some(path) => Some(BufWriter::new(
				File::create(path).with_context(|| format!("creating the stats file {}", path.display()))?,
			)),
			None => None,
		};
		runtime.measure_stages();
		Ok(Some(Monitor::start(
			Some(runtime.clone()),
			self.stats,
			file,
			SAMPLE_EVERY,
			SAMPLES_PER_REPORT,
		)))
	}
}

/// The running monitor. [`finish`](Self::finish) stops it and logs the summary;
/// dropping it stops it silently.
pub struct Monitor {
	stop: Option<oneshot::Sender<()>>,
	task: JoinHandle<Result<Vec<String>>>,
	report: bool,
}

impl Monitor {
	fn start(
		runtime: Option<TilesRuntime>,
		report: bool,
		file: Option<BufWriter<File>>,
		every: Duration,
		per_report: u32,
	) -> Self {
		let (stop, stopped) = oneshot::channel();
		let task = tokio::spawn(sample(runtime, report, file, every, per_report, stopped));
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
			for line in summary {
				log::info!("stats: {line}");
			}
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
	runtime: Option<TilesRuntime>,
	report: bool,
	mut file: Option<BufWriter<File>>,
	every: Duration,
	per_report: u32,
	mut stopped: oneshot::Receiver<()>,
) -> Result<Vec<String>> {
	let stages = || runtime.as_ref().map_or_default(TilesRuntime::stage_stats);
	let cores = available_cores();
	let first = ResourceUsage::read();
	let mut last = first;
	let mut last_report = first;
	let mut last_report_stages = Vec::new();
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
		let now_stages = stages();
		let rates = now.since(&last, cores);
		peak_rss = peak_rss.max(now.rss_bytes.unwrap_or(0));
		// The last interval is cut short, and a share over a few milliseconds
		// would skew the median.
		if !stopping && let Some(cpu) = rates.cpu {
			shares.push(cpu);
		}
		if let Some(file) = &mut file {
			writeln!(file, "{}", json_line(&now, &first, &rates, cores, &now_stages)).context("writing the stats file")?;
			file.flush().context("writing the stats file")?;
		}
		count += 1;
		if report && !stopping && count.is_multiple_of(per_report) {
			let seconds = now.since(&last_report, cores).seconds;
			log::info!("stats: {}", report_line(&now.since(&last_report, cores), &now, cores));
			let rates = stage_rates(&now_stages, &last_report_stages, seconds);
			if !rates.is_empty() {
				log::info!("stats: {}", stages_line(&rates));
			}
			last_report = now;
			last_report_stages.clone_from(&now_stages);
		}
		last = now;
		if stopping {
			let peak = now.peak_rss_bytes.unwrap_or(0).max(peak_rss);
			let total = now.since(&first, cores);
			let mut lines = vec![summary_line(&total, &now, &first, &mut shares, peak, cores)];
			let stage_rates = stage_rates(&now_stages, &[], total.seconds);
			let process_cores = total.cpu.map(|cpu| cpu * cores as f64);
			lines.extend(stage_table(
				&stage_rates,
				now_stages.iter().map(|stage| stage.tiles),
				process_cores,
			));
			lines.extend(bottleneck(&stage_rates, total.cpu));
			return Ok(lines);
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
fn json_line(
	now: &ResourceUsage,
	first: &ResourceUsage,
	rates: &UsageRates,
	cores: usize,
	stages: &[StageStats],
) -> String {
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
	let mut body: Vec<String> = fields.iter().map(|(key, value)| format!("\"{key}\":{value}")).collect();
	if !stages.is_empty() {
		let stages: Vec<String> = stages
			.iter()
			.map(|stage| {
				format!(
					"{{\"name\":{},\"input\":{},\"tiles\":{},\"cpu_seconds\":{},\"waited_seconds\":{}}}",
					string(&stage.name),
					stage.input.as_deref().map_or("null".to_string(), string),
					stage.tiles,
					fraction(Some(stage.cpu.as_secs_f64())),
					fraction(Some(stage.waited.as_secs_f64())),
				)
			})
			.collect();
		body.push(format!("\"stages\":[{}]", stages.join(",")));
	}
	format!("{{{}}}", body.join(","))
}

/// What one stage did over a stretch of time.
#[derive(Debug, PartialEq)]
struct StageRates {
	name: String,
	input: Option<String>,
	/// Cores its own work kept busy, on average.
	cores: f64,
	/// Requests waiting for its tiles, on average.
	waited_on: f64,
}

/// The rates of each stage between `before` and `now`, `seconds` apart.
/// Stages are matched by position, as they are only ever added.
fn stage_rates(now: &[StageStats], before: &[StageStats], seconds: f64) -> Vec<StageRates> {
	if seconds <= 0.0 {
		return Vec::new();
	}
	now.iter()
		.enumerate()
		.map(|(index, stage)| {
			let (cpu, waited) = before
				.get(index)
				.map_or((Duration::ZERO, Duration::ZERO), |before| (before.cpu, before.waited));
			StageRates {
				name: stage.name.clone(),
				input: stage.input.clone(),
				cores: stage.cpu.saturating_sub(cpu).as_secs_f64() / seconds,
				waited_on: stage.waited.saturating_sub(waited).as_secs_f64() / seconds,
			}
		})
		.collect()
}

/// One logged line for the stages of a pipeline.
fn stages_line(rates: &[StageRates]) -> String {
	let parts: Vec<String> = rates
		.iter()
		.map(|stage| {
			format!(
				"{} {:.1} cores, waited on {:.2}",
				stage.name, stage.cores, stage.waited_on
			)
		})
		.collect();
	format!("stages: {}", parts.join("; "))
}

/// The stages as a table: tiles delivered, cores busy, and how many requests
/// waited for their tiles. CPU time no stage accounts for, such as the
/// runtime's own, is the last row.
fn stage_table(rates: &[StageRates], tiles: impl Iterator<Item = u64>, process_cores: Option<f64>) -> Vec<String> {
	if rates.is_empty() {
		return Vec::new();
	}
	let width = rates.iter().map(|stage| stage.name.len()).max().unwrap_or(0).max(5);
	let mut lines = vec![format!(
		"{:width$}  {:>12}  {:>6}  {:>9}",
		"stage", "tiles", "cores", "waited on"
	)];
	for (stage, tiles) in rates.iter().zip(tiles) {
		lines.push(format!(
			"{:width$}  {tiles:>12}  {:>6.1}  {:>9.2}",
			stage.name, stage.cores, stage.waited_on
		));
	}
	if let Some(process_cores) = process_cores {
		let counted: f64 = rates.iter().map(|stage| stage.cores).sum();
		lines.push(format!(
			"{:width$}  {:>12}  {:>6.1}",
			"other",
			"",
			(process_cores - counted).max(0.0)
		));
	}
	lines
}

/// A hint at what limits the run, if the numbers point somewhere.
///
/// When most cores are busy, the run is limited by computation, and the stage
/// using the most of them is where it goes. Otherwise the stage that holds the
/// others up is the one waited on most beyond what it waits on its own input.
fn bottleneck(rates: &[StageRates], cpu: Option<f64>) -> Option<String> {
	if cpu.is_some_and(|cpu| cpu >= 0.8) {
		let busiest = rates.iter().max_by(|a, b| a.cores.total_cmp(&b.cores))?;
		return Some(format!(
			"the cores are busy; {} uses the most of them ({:.1} cores)",
			busiest.name, busiest.cores
		));
	}
	let waited_on_input = |stage: &StageRates| {
		stage.input.as_ref().map_or(0.0, |input| {
			rates
				.iter()
				.find(|other| &other.name == input)
				.map_or(0.0, |input| input.waited_on)
		})
	};
	let (stage, margin) = rates
		.iter()
		.map(|stage| (stage, stage.waited_on - waited_on_input(stage)))
		.max_by(|a, b| a.1.total_cmp(&b.1))?;
	(margin >= 0.1).then(|| {
		format!(
			"likely bottleneck: {}, waited on {:.2} while it waited on its input {:.2}",
			stage.name,
			stage.waited_on,
			waited_on_input(stage)
		)
	})
}

/// `text` as a JSON string.
fn string(text: &str) -> String {
	let mut quoted = String::with_capacity(text.len() + 2);
	quoted.push('"');
	for character in text.chars() {
		match character {
			'"' => quoted.push_str("\\\""),
			'\\' => quoted.push_str("\\\\"),
			c if c.is_control() => {
				let _ = write!(quoted, "\\u{:04x}", u32::from(c));
			}
			c => quoted.push(c),
		}
	}
	quoted.push('"');
	quoted
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
		let line = json_line(&now, &first, &rates, 4, &[]);
		assert_eq!(
			line,
			r#"{"seconds":2.000,"cores":4,"cpu":0.250,"iowait":null,"cpu_seconds":2.000,"rss_bytes":300,"peak_rss_bytes":300,"disk_read_bytes":100,"disk_written_bytes":0}"#
		);
		JsonObject::parse_str(&line)?;
		Ok(())
	}

	fn stage(name: &str, input: Option<&str>, cpu_seconds: u64, waited_seconds: u64) -> StageStats {
		StageStats {
			name: name.to_string(),
			input: input.map(str::to_string),
			cpu: Duration::from_secs(cpu_seconds),
			tiles: 100,
			waited: Duration::from_secs(waited_seconds),
		}
	}

	/// A pipeline held up by its middle stage, over 10 seconds.
	fn held_up() -> Vec<StageRates> {
		let stages = [
			stage("from_container", None, 3, 1),
			stage("dem_hillshade", Some("from_container"), 60, 8),
			stage("raster_format", Some("dem_hillshade"), 50, 2),
			stage("write", Some("raster_format"), 2, 0),
		];
		stage_rates(&stages, &[], 10.0)
	}

	#[test]
	fn stage_rates_cover_the_interval() {
		let before = [stage("a", None, 10, 2)];
		let now = [stage("a", None, 30, 7), stage("b", Some("a"), 5, 0)];
		assert_eq!(
			stage_rates(&now, &before, 5.0),
			vec![
				StageRates {
					name: "a".to_string(),
					input: None,
					cores: 4.0,
					waited_on: 1.0
				},
				StageRates {
					name: "b".to_string(),
					input: Some("a".to_string()),
					cores: 1.0,
					waited_on: 0.0
				},
			]
		);
	}

	#[test]
	fn the_table_lists_each_stage_and_what_is_left() {
		let table = stage_table(&held_up(), [10, 20, 30, 40].into_iter(), Some(12.0));
		assert_eq!(
			table,
			vec![
				"stage                  tiles   cores  waited on",
				"from_container            10     0.3       0.10",
				"dem_hillshade             20     6.0       0.80",
				"raster_format             30     5.0       0.20",
				"write                     40     0.2       0.00",
				"other                            0.5",
			]
		);
	}

	#[test]
	fn the_bottleneck_is_waited_on_more_than_it_waits() {
		assert_eq!(
			bottleneck(&held_up(), Some(0.4)).as_deref(),
			Some("likely bottleneck: dem_hillshade, waited on 0.80 while it waited on its input 0.10")
		);
		assert_eq!(
			bottleneck(&held_up(), Some(0.9)).as_deref(),
			Some("the cores are busy; dem_hillshade uses the most of them (6.0 cores)")
		);
		let idle = stage_rates(&[stage("read", None, 1, 0)], &[], 10.0);
		assert_eq!(bottleneck(&idle, Some(0.1)), None);
	}

	#[test]
	fn a_json_line_lists_the_stages() -> Result<()> {
		let start = Instant::now();
		let first = reading(start, 0, 1, 0, 0);
		let now = reading(start + Duration::from_secs(1), 1, 1, 0, 0);
		let stages = [stage("from_\"x\"", None, 1, 2)];
		let line = json_line(&now, &first, &now.since(&first, 1), 1, &stages);
		assert!(
			line.ends_with(
				r#","stages":[{"name":"from_\"x\"","input":null,"tiles":100,"cpu_seconds":1.000,"waited_seconds":2.000}]}"#
			),
			"{line}"
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
			None,
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
