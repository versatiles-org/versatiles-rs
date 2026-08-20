//! Per-tile error handling for streams.
//!
//! One tile failing is not the same event as a source failing to open, and the
//! difference is what [`record_errors`](TileStreamErrorExt::record_errors)
//! exists for. Whether a bad tile stops the run is the caller's policy, held on
//! [`TilesRuntime`]: `convert` sets `abort_on_error` and fails once the stream
//! has drained, the server leaves it off and keeps serving. Producers do not
//! need to know which of those they are inside.
//!
//! This lives in `versatiles_container` rather than beside the other
//! `TileStream` combinators because it needs [`TilesRuntime`], and
//! `versatiles_core` — where `TileStream` is defined — cannot depend on this
//! crate.

use anyhow::Result;
use versatiles_core::TileStream;

use crate::TilesRuntime;

/// Routes per-tile failures to the runtime instead of unwrapping them.
pub trait TileStreamErrorExt<'a, T> {
	/// Records every `Err` on `runtime` and drops that tile, yielding only the
	/// tiles that succeeded.
	///
	/// The alternative in this codebase used to be
	/// [`unwrap_results`](versatiles_core::TileStream::unwrap_results), which
	/// panics — a whole process taken down by one unreadable tile, with no way
	/// for the caller to decide otherwise. Use `unwrap_results` only where the
	/// `Err` variant is unreachable by construction.
	///
	/// `label` names the producer and is what the user sees in front of the
	/// error, e.g. `"raster_flatten"`.
	#[must_use]
	fn record_errors(self, runtime: &TilesRuntime, label: &str) -> TileStream<'a, T>;
}

impl<'a, T> TileStreamErrorExt<'a, T> for TileStream<'a, Result<T>>
where
	T: Send + 'a,
{
	fn record_errors(self, runtime: &TilesRuntime, label: &str) -> TileStream<'a, T> {
		let runtime = runtime.clone();
		let label = label.to_string();
		self.filter_map(move |coord, result| match result {
			Ok(item) => Some(item),
			Err(error) => {
				runtime.record_error(&format!("{label} tile {}/{}/{}", coord.level, coord.x, coord.y), &error);
				None
			}
		})
	}
}

#[cfg(test)]
mod tests {
	use std::sync::{Arc, Mutex};

	use anyhow::anyhow;
	use versatiles_core::{Blob, TileCoord};

	use super::*;

	fn stream(items: Vec<(u8, std::result::Result<&'static str, &'static str>)>) -> TileStream<'static, Result<Blob>> {
		TileStream::from_vec(
			items
				.into_iter()
				.map(|(x, item)| {
					let coord = TileCoord::new(3, u32::from(x), 0).unwrap();
					(coord, item.map(Blob::from).map_err(|e| anyhow!("{e}")))
				})
				.collect(),
		)
	}

	#[tokio::test]
	async fn failing_tiles_are_dropped_and_counted() {
		let runtime = TilesRuntime::new_silent();
		let items = stream(vec![(0, Ok("a")), (1, Err("broken")), (2, Ok("c"))])
			.record_errors(&runtime, "test_op")
			.to_vec()
			.await;

		assert_eq!(
			items.iter().map(|(_, b)| b.as_str()).collect::<Vec<_>>(),
			["a", "c"],
			"the good tiles keep flowing"
		);
		assert_eq!(runtime.error_count(), 1);
	}

	#[tokio::test]
	async fn a_dropped_tile_only_ends_the_run_when_the_caller_asked() {
		// The same stream, the same failure, two answers — which is the point of
		// putting the policy on the runtime rather than in the producer.
		let lenient = TilesRuntime::new_silent();
		let _ = stream(vec![(0, Err("broken"))])
			.record_errors(&lenient, "test_op")
			.to_vec()
			.await;
		assert!(!lenient.had_errors(), "a server keeps going");

		let strict = TilesRuntime::new_silent();
		strict.set_abort_on_error(true);
		let _ = stream(vec![(0, Err("broken"))])
			.record_errors(&strict, "test_op")
			.to_vec()
			.await;
		assert!(strict.had_errors(), "a conversion has something to fail on");
	}

	#[tokio::test]
	async fn the_error_names_the_producer_and_the_tile() {
		let runtime = TilesRuntime::new_silent();
		let seen = Arc::new(Mutex::new(Vec::new()));
		let sink = Arc::clone(&seen);
		runtime.events().subscribe(move |event| {
			if let crate::Event::Error { message } = event {
				sink.lock().unwrap().push(message.clone());
			}
		});

		let _ = stream(vec![(5, Err("broken"))])
			.record_errors(&runtime, "raster_flatten")
			.to_vec()
			.await;

		let messages = seen.lock().unwrap();
		let message = messages.first().expect("an error event was emitted");
		assert!(message.contains("raster_flatten"), "names the producer: {message}");
		assert!(message.contains("3/5/0"), "names the tile: {message}");
		assert!(message.contains("broken"), "carries the reason: {message}");
	}
}
