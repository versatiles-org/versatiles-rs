use log::{max_level, LevelFilter};
use std::io::Write;
use std::time::{Duration, SystemTime};
use term_size::dimensions_stdout;

const STEP_SIZE: Duration = Duration::from_millis(500);

pub struct ProgressBar {
	max_value: u64,
	message: String,
	start: SystemTime,
	next_update: SystemTime,
	value: u64,
	finished: bool,
	visible: bool,
}

#[allow(dead_code)]
impl ProgressBar {
	pub fn new(message: &str, max_value: u64) -> Self {
		let now = SystemTime::now();
		let mut progress = ProgressBar {
			max_value,
			message: message.to_string(),
			start: now,
			next_update: now.checked_sub(STEP_SIZE).unwrap(),
			value: 0,
			finished: false,
			visible: max_level() >= LevelFilter::Info,
		};
		progress.update();
		progress
	}
	pub fn set_position(&mut self, value: u64) {
		self.value = value;
		self.update();
	}
	pub fn inc(&mut self, value: u64) {
		self.value += value;
		self.update();
	}
	pub fn set_visible(&mut self, visible: bool) {
		self.visible = visible;
	}
	fn update(&mut self) {
		let now = SystemTime::now();
		if now < self.next_update {
			return;
		}
		self.next_update = now.checked_add(STEP_SIZE).unwrap();

		if self.visible {
			self.draw();
		}
	}
	pub fn finish(&mut self) {
		self.finished = true;
		self.value = self.max_value;

		if self.visible {
			self.draw();
			println!();
		}
	}
	fn draw(&mut self) {
		let size = dimensions_stdout();
		if size.is_none() {
			return;
		}

		let width: i64 = size.unwrap().0 as i64;

		let duration = SystemTime::now().duration_since(self.start).unwrap();
		let progress = self.value as f64 / self.max_value as f64;
		let time_left = Duration::from_secs_f64(duration.as_secs_f64() / (progress + 1e-3) * (1.0 - progress));
		let speed = self.value as f64 / duration.as_secs_f64();

		let col1 = self.message.to_string();
		let col2 = format!(
			"{:>15}/{:<15}{:.2}%",
			format_big_number(self.value),
			format_big_number(self.max_value),
			progress * 100.0
		);
		let col3 = format!(
			"{}/s {:>15} {:>15}",
			format_big_number(speed as u64),
			format_duration(duration),
			format_duration(time_left)
		);

		let length1 = col1.len() as i64;
		let length2 = col2.len() as i64;
		let length3 = col3.len() as i64;

		let space1 = 0.min((width - length2) / 2 - length1);
		let space2 = 0.min(width - (length1 + space1 + length2 + length3));

		let line = format!(
			"\r{}{}{}{}{}",
			col1,
			" ".repeat(space1 as usize),
			col2,
			" ".repeat(space2 as usize),
			col3
		);
		let pos = (line.len() as f64 * progress).round() as usize;

		print!("\r\x1B[7m{}\x1B[0m{}", &line[0..pos], &line[pos..]);
		std::io::stdout().flush().unwrap();
	}
}

fn format_duration(duration: Duration) -> String {
	let mut t = duration.as_secs();
	let seconds = t % 60;
	t /= 60;
	let minutes = t % 60;
	t /= 60;
	let hours = t % 24;
	t /= 24;
	if t > 0 {
		let days = t;
		format!("{days}d {hours:02}:{minutes:02}:{seconds:02}")
	} else {
		format!("{hours:02}:{minutes:02}:{seconds:02}")
	}
}

fn format_big_number(value: u64) -> String {
	value
		.to_string()
		.as_bytes()
		.rchunks(3)
		.rev()
		.map(std::str::from_utf8)
		.collect::<Result<Vec<&str>, _>>()
		.unwrap()
		.join("'")
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::time::Duration;

	#[test]
	fn format() {
		assert_eq!(format_big_number(123456789), "123'456'789");
		assert_eq!(format_big_number(1234567890), "1'234'567'890");
		assert_eq!(format_duration(Duration::from_secs(1)), "00:00:01");
		assert_eq!(format_duration(Duration::from_secs(60)), "00:01:00");
		assert_eq!(format_duration(Duration::from_secs(60 * 60)), "01:00:00");
		assert_eq!(format_duration(Duration::from_secs(60 * 60 * 24)), "1d 00:00:00");
	}

	#[test]
	fn progress_bar() {
		let mut progress = ProgressBar::new("hallo", 100);
		progress.set_visible(true);
		progress.set_position(1);
		progress.inc(1);
		progress.finish();
	}
}
