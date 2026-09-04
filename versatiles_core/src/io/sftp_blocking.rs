//! The bridge between the synchronous writer traits and russh's async API.
//!
//! [`DataWriterTrait`](super::DataWriterTrait) is synchronous — `fn append`,
//! `fn set_position` — and is implemented by the file and blob writers as well
//! as the SFTP one, so it cannot be made async without rewriting every sink in
//! `versatiles_container`. russh, meanwhile, is async all the way down. One of
//! the two has to bend, and until the trait itself is converted the bend
//! happens here.
//!
//! This is not a regression: the ssh2 implementation blocked its caller too
//! (libssh2 performs blocking socket I/O), so the same threads block for the
//! same reasons. What changes is *where* the blocking is written down — one
//! module, rather than smeared through every writer.
//!
//! # Why a dedicated thread
//!
//! `Runtime::block_on` panics when it is called from inside another runtime,
//! and the sinks are driven from async code, so blocking on the caller's own
//! runtime is not an option.
//!
//! Instead one background thread owns a `current_thread` runtime, and
//! [`block_on`] enters that runtime's context before polling the future on the
//! *calling* thread with `futures::executor::block_on`. Two things fall out of
//! that split:
//!
//! - Timers and sockets created by the future register with the background
//!   runtime, which is parked driving them, so they still fire. The same is
//!   true of the transport task russh spawns at connect time — every SFTP
//!   session therefore lives on the runtime that opened it, which russh
//!   requires.
//! - The future is polled in place, so it may borrow. Handing it to
//!   `Handle::spawn` instead would demand `Send + 'static`, which for a write
//!   means copying the caller's buffer for no reason.
//!
//! The runtime is process-global rather than per-writer: one thread drives
//! every SFTP session, instead of one thread per open remote file.

use std::{future::Future, sync::OnceLock};

use anyhow::{Result, anyhow};
use tokio::runtime::{Builder, Handle};

/// The handle to the background SFTP runtime, started on first use.
fn runtime() -> Result<&'static Handle> {
	static RUNTIME: OnceLock<Result<Handle, String>> = OnceLock::new();

	RUNTIME
		.get_or_init(|| {
			let (sender, receiver) = std::sync::mpsc::sync_channel(0);

			std::thread::Builder::new()
				.name("sftp-runtime".into())
				.spawn(move || {
					let runtime = match Builder::new_current_thread().enable_all().build() {
						Ok(runtime) => runtime,
						Err(e) => {
							// The receiver is still waiting; report and stop.
							let _ = sender.send(Err(format!("failed to start the SFTP runtime: {e}")));
							return;
						}
					};
					if sender.send(Ok(runtime.handle().clone())).is_err() {
						return; // nobody is waiting for us any more
					}
					// Park the thread inside the runtime so spawned tasks keep
					// being polled. The channel never yields a value; the thread
					// ends with the process.
					runtime.block_on(std::future::pending::<()>());
				})
				.map_err(|e| format!("failed to spawn the SFTP runtime thread: {e}"))?;

			receiver
				.recv()
				.map_err(|_| "the SFTP runtime thread stopped before it was ready".to_owned())?
		})
		.as_ref()
		.map_err(|e| anyhow!(e.clone()))
}

/// Hand `future` to the SFTP runtime and let it finish on its own.
///
/// For cleanup that a `Drop` impl cannot await. A failure to start the runtime
/// is logged rather than returned: the caller is a destructor and has nowhere
/// to report to.
pub fn spawn<F>(future: F)
where
	F: Future<Output = ()> + Send + 'static,
{
	match runtime() {
		Ok(handle) => drop(handle.spawn(future)),
		Err(e) => log::debug!("could not run SFTP cleanup: {e:#}"),
	}
}

/// Run `future` to completion in the SFTP runtime's context, blocking the
/// calling thread until it ends.
///
/// # Errors
/// Returns an error if the SFTP runtime cannot be started.
pub fn block_on<F: Future>(future: F) -> Result<F::Output> {
	// Held for the whole poll: anything the future registers with the runtime
	// (a timer, a socket, a spawned task) has to find it in the thread-local
	// context at the moment it is created.
	let _guard = runtime()?.enter();
	Ok(futures::executor::block_on(future))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn runs_a_future_to_completion() {
		assert_eq!(block_on(async { 1 + 1 }).unwrap(), 2);
	}

	/// The reason this module exists: the sinks call it from inside a runtime,
	/// where `Runtime::block_on` would panic.
	#[test]
	fn works_from_inside_another_runtime() {
		let outer = Builder::new_current_thread().enable_all().build().unwrap();
		let value = outer.block_on(async {
			tokio::task::spawn_blocking(|| block_on(async { "nested" }).unwrap())
				.await
				.unwrap()
		});
		assert_eq!(value, "nested");
	}

	/// Sequential calls share one runtime rather than starting a thread each.
	#[test]
	fn the_runtime_is_reused() {
		let first = runtime().unwrap();
		let second = runtime().unwrap();
		assert!(std::ptr::eq(first, second));
	}

	/// Work actually runs concurrently on the shared runtime, so one blocked
	/// caller does not stall another.
	#[test]
	fn concurrent_callers_do_not_serialise() {
		let threads = (0..4)
			.map(|i| {
				std::thread::spawn(move || {
					block_on(async move {
						tokio::time::sleep(std::time::Duration::from_millis(200)).await;
						i
					})
					.unwrap()
				})
			})
			.collect::<Vec<_>>();

		let started = std::time::Instant::now();
		let mut results = threads.into_iter().map(|t| t.join().unwrap()).collect::<Vec<_>>();
		results.sort_unstable();

		assert_eq!(results, vec![0, 1, 2, 3]);
		assert!(
			started.elapsed() < std::time::Duration::from_millis(700),
			"four 200 ms sleeps took {:?}; they should have overlapped",
			started.elapsed()
		);
	}
}
