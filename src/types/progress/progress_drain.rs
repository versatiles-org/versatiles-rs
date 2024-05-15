use super::traits::ProgressTrait;

pub struct ProgressDrain {}

impl ProgressTrait for ProgressDrain {
	fn new() -> Self {
		Self {}
	}
	fn init(&mut self, _message: &str, _max_value: u64) {}
	fn set_position(&mut self, _value: u64) {}
	fn inc(&mut self, _value: u64) {}
	fn finish(&mut self) {}
	fn remove(&mut self) {}
}
