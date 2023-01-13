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
}

impl ProgressBar {
	pub fn new(message: &str, max_value: u64) -> Self {
		ProgressBar {
			max_value,
			message: message.to_string(),
			start: SystemTime::now(),
			next_update: SystemTime::now().checked_add(STEP_SIZE).unwrap(),
			value: 0,
			finished: false,
		}
	}
	pub fn set_position(&mut self, value: u64) {
		self.value = value;
		self.update();
	}
	pub fn inc(&mut self, value: u64) {
		self.value += value;
		self.update();
	}
	fn update(&mut self) {
		if SystemTime::now() < self.next_update {
			return;
		}
		self.next_update = SystemTime::now().checked_add(STEP_SIZE).unwrap();
		self.draw();
	}
	fn draw(&mut self) {
		let width = dimensions_stdout().unwrap().0;

		let duration = SystemTime::now().duration_since(self.start).unwrap();
		let progress = self.value as f64 / self.max_value as f64;
		let time_left =
			Duration::from_secs_f64(duration.as_secs_f64() / (progress + 0e-3) * (1.0 - progress));
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

		let space1 = (width - col2.len()) / 2 - col1.len();
		let space2 = width - (col1.len() + space1 + col2.len() + col3.len());

		let line = format!(
			"\r{}{}{}{}{}",
			col1,
			" ".repeat(space1),
			col2,
			" ".repeat(space2),
			col3
		);
		let pos = (line.len() as f64 * progress).round() as usize;

		eprint!("\r\x1B[7m{}\x1B[0m{}", &line[0..pos], &line[pos..]);
	}
	pub fn finish(&mut self) {
		self.finished = true;
		self.value = self.max_value;
		self.draw();
		eprintln!();
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
		format!("{}d {:02}:{:02}:{:02}", days, hours, minutes, seconds)
	} else {
		format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
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
