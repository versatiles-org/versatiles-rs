use indicatif;
use std::time::{Duration, SystemTime};

pub struct ProgressBar {
	bar: indicatif::ProgressBar,
	next_progress_update: SystemTime,
	value: u64,
}

impl ProgressBar {
	pub fn new(message: &str, max_value: u64) -> Self {
		let template = format!(
			"{}: {}",
			message,
			"{wide_bar:0.white/dim.white} {pos:>9}/{len:9} {per_sec:18} {elapsed_precise} {eta_precise}"
		);
		let bar = indicatif::ProgressBar::new(max_value);
		bar.set_style(
			indicatif::ProgressStyle::with_template(template.as_str())
				.unwrap()
				.progress_chars("██▁"),
		);

		let bar = ProgressBar {
			bar,
			next_progress_update: SystemTime::now(),
			value: 0,
		};
		return bar;
	}
	pub fn set_position(&mut self, value: u64) {
		self.value = value;
		self.update()
	}
	pub fn inc(&mut self, value: u64) {
		self.value += value;
		self.update()
	}
	pub fn finish(&mut self) {
		self.force_update();
		self.bar.abandon();
	}
	fn set_next_progress_update(&mut self) {
		self.next_progress_update += Duration::from_secs(1);
	}
	fn update(&mut self) {
		if SystemTime::now() >= self.next_progress_update {
			self.bar.set_position(self.value);
			self.set_next_progress_update();
		}
	}
	fn force_update(&mut self) {
		self.bar.set_position(self.value);
		self.set_next_progress_update();
	}
}
