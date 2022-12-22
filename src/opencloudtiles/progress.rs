use indicatif;

pub struct ProgressBar {
	bar: indicatif::ProgressBar,
}

impl ProgressBar {
	pub fn new(message: &str, max_value: u64) -> Self {
		let bar = indicatif::ProgressBar::new(max_value);
		bar.set_style(
			indicatif::ProgressStyle::with_template(
				format!(
				"{}: {}",
				message,
				"{wide_bar:0.white/dim.white} {pos:>9}/{len:9} {per_sec:18} {elapsed_precise} {eta_precise}"
			)
				.as_str(),
			)
			.unwrap()
			.progress_chars("██▁"),
		);

		return ProgressBar { bar };
	}
	pub fn set_position(&self, value: u64) {
		self.bar.set_position(value);
	}
	pub fn inc(&self, value: u64) {
		self.bar.inc(value);
	}
	pub fn finish(&self) {
		self.bar.abandon();
	}
}
