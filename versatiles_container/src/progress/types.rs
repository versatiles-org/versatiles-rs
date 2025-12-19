#[derive(Debug, Clone)]
pub struct ProgressId(pub u32);

#[derive(Debug, Clone)]
pub struct ProgressState {
	pub id: ProgressId,
	pub message: String,
	pub position: u64,
	pub total: u64,
	pub start: std::time::Instant,
	pub next_draw: std::time::Instant,
	pub finished: bool,
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_progress_id_creation() {
		let id1 = ProgressId(1);
		let id2 = ProgressId(2);
		assert_eq!(id1.0, 1);
		assert_eq!(id2.0, 2);
	}

	#[test]
	fn test_progress_id_clone() {
		let id1 = ProgressId(42);
		let id2 = id1.clone();
		assert_eq!(id1.0, id2.0);
	}

	#[test]
	fn test_progress_state_creation() {
		let now = std::time::Instant::now();
		let state = ProgressState {
			id: ProgressId(1),
			message: "Testing".to_string(),
			position: 50,
			total: 100,
			start: now,
			next_draw: now,
			finished: false,
		};

		assert_eq!(state.id.0, 1);
		assert_eq!(state.message, "Testing");
		assert_eq!(state.position, 50);
		assert_eq!(state.total, 100);
		assert!(!state.finished);
	}

	#[test]
	fn test_progress_state_clone() {
		let now = std::time::Instant::now();
		let state1 = ProgressState {
			id: ProgressId(5),
			message: "Clone test".to_string(),
			position: 25,
			total: 50,
			start: now,
			next_draw: now,
			finished: true,
		};

		let state2 = state1.clone();
		assert_eq!(state1.id.0, state2.id.0);
		assert_eq!(state1.message, state2.message);
		assert_eq!(state1.position, state2.position);
		assert_eq!(state1.total, state2.total);
		assert_eq!(state1.finished, state2.finished);
	}

	#[test]
	fn test_progress_state_finished_flag() {
		let now = std::time::Instant::now();
		let mut state = ProgressState {
			id: ProgressId(10),
			message: "Finish test".to_string(),
			position: 0,
			total: 100,
			start: now,
			next_draw: now,
			finished: false,
		};

		assert!(!state.finished);
		state.finished = true;
		assert!(state.finished);
	}
}
