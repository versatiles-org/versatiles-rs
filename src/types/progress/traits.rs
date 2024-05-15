pub fn get_progress_bar(message: &str, max_value: u64) -> Box<dyn ProgressTrait> {
	#[cfg(all(feature = "full", not(test)))]
	let mut progress = super::progress_bar::ProgressBar::new();
	#[cfg(any(not(feature = "full"), test))]
	let mut progress = super::progress_drain::ProgressDrain::new();
	progress.init(message, max_value);
	Box::new(progress)
}

pub trait ProgressTrait: Send + Sync {
	fn new() -> Self
	where
		Self: Sized;
	fn init(&mut self, message: &str, max_value: u64);
	fn set_position(&mut self, value: u64);
	fn inc(&mut self, value: u64);
	fn finish(&mut self);
	fn remove(&mut self);
}
