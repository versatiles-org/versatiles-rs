//! Telling a tile request to stop (#265).
//!
//! A caller that no longer wants a tile can drop the future, and that stops the
//! part of the work that is a future. Work on the blocking pool keeps running:
//! dropping a `JoinHandle` does not stop the closure. A [`CancelToken`] reaches
//! that work too.
//!
//! The token travels with the request rather than through every signature. The
//! caller runs the request inside [`CancelToken::scope`], and any code polled
//! as part of it reads the token with [`CancelToken::current`], however deeply
//! it is nested. Operations check it where they already pause: between blocks,
//! between chunks, before opening a dataset. One that never checks behaves as
//! before.
//!
//! ```
//! use versatiles_core::CancelToken;
//!
//! # #[tokio::main] async fn main() {
//! let token = CancelToken::new();
//! let request = token.clone().scope(async {
//!     // somewhere deep inside an operation
//!     CancelToken::current().check()
//! });
//! token.cancel();
//! assert!(request.await.is_err());
//! # }
//! ```
//!
//! # Where the token does not reach on its own
//!
//! - **`spawn_blocking` and `tokio::spawn`.** A new task does not inherit it.
//!   Capture it with [`CancelToken::current`] before spawning and move it in;
//!   blocking code can re-enter it with [`CancelToken::sync_scope`]:
//!
//!   ```
//!   # use versatiles_core::CancelToken;
//!   # async fn f() -> anyhow::Result<()> {
//!   let cancel = CancelToken::current();
//!   tokio::task::spawn_blocking(move || cancel.sync_scope(|| CancelToken::current().check())).await??;
//!   # Ok(()) }
//!   ```
//! - **Work shared between requests.** A cache that merges concurrent requests
//!   for one key into one computation must not run that computation under the
//!   token of whichever request started it, or cancelling that request fails
//!   the others. Run it under [`CancelToken::never`] instead.
//!
//! Without a scope, [`CancelToken::current`] is a token that is never
//! cancelled, so code that checks costs nothing to callers that do not use
//! tokens, and bulk work such as a conversion is never stopped by one.

use std::{
	fmt,
	future::Future,
	sync::{
		Arc,
		atomic::{AtomicBool, Ordering},
	},
	time::Instant,
};

use anyhow::Result;

tokio::task_local! {
	static CURRENT: CancelToken;
}

/// A request's permission to keep working. See the [module docs](self).
///
/// Clones share one state: cancelling any of them cancels all.
#[derive(Clone, Debug, Default)]
pub struct CancelToken(Option<Arc<State>>);

#[derive(Debug)]
struct State {
	cancelled: AtomicBool,
	deadline: Option<Instant>,
}

impl CancelToken {
	/// A token that is cancelled when [`cancel`](Self::cancel) is called.
	#[must_use]
	pub fn new() -> Self {
		Self::from_state(None)
	}

	/// A token that is cancelled when `deadline` passes, or when
	/// [`cancel`](Self::cancel) is called before that.
	///
	/// The deadline is the caller's: the library sets none of its own, because
	/// a wall-clock limit would let the same pipeline pass on one machine and
	/// fail on another.
	#[must_use]
	pub fn with_deadline(deadline: Instant) -> Self {
		Self::from_state(Some(deadline))
	}

	fn from_state(deadline: Option<Instant>) -> Self {
		Self(Some(Arc::new(State {
			cancelled: AtomicBool::new(false),
			deadline,
		})))
	}

	/// A token that is never cancelled. [`cancel`](Self::cancel) does nothing.
	#[must_use]
	pub fn never() -> Self {
		Self(None)
	}

	/// Cancels this token and every clone of it.
	pub fn cancel(&self) {
		if let Some(state) = &self.0 {
			state.cancelled.store(true, Ordering::Relaxed);
		}
	}

	/// Whether the token has been cancelled or its deadline has passed.
	#[must_use]
	pub fn is_cancelled(&self) -> bool {
		self.0.as_ref().is_some_and(|state| {
			state.cancelled.load(Ordering::Relaxed) || state.deadline.is_some_and(|deadline| Instant::now() >= deadline)
		})
	}

	/// Returns [`Cancelled`] once the token is cancelled.
	///
	/// # Errors
	/// Returns an error wrapping [`Cancelled`] if the token is cancelled.
	pub fn check(&self) -> Result<()> {
		if self.is_cancelled() {
			Err(Cancelled.into())
		} else {
			Ok(())
		}
	}

	/// The token of the request being worked on, or [`never`](Self::never)
	/// outside of any [`scope`](Self::scope).
	#[must_use]
	pub fn current() -> Self {
		CURRENT.try_with(Clone::clone).unwrap_or_default()
	}

	/// Runs `future` with this token as [`current`](Self::current).
	///
	/// The token is set only while `future` is polled, so requests running
	/// side by side, even on one task, each see their own.
	pub fn scope<F: Future>(self, future: F) -> impl Future<Output = F::Output> {
		CURRENT.scope(self, future)
	}

	/// Runs the synchronous `f` with this token as [`current`](Self::current):
	/// the way back into a request's scope for code on the blocking pool.
	pub fn sync_scope<R>(self, f: impl FnOnce() -> R) -> R {
		CURRENT.sync_scope(self, f)
	}

	/// A guard that cancels this token when it is dropped.
	///
	/// Held by whoever waits for the request: when the request is abandoned,
	/// for example because a client disconnected and its handler was dropped,
	/// the work that outlives the future learns about it.
	#[must_use]
	pub fn cancel_on_drop(&self) -> CancelOnDrop {
		CancelOnDrop(self.clone())
	}
}

/// Cancels its token when dropped. See [`CancelToken::cancel_on_drop`].
#[derive(Debug)]
pub struct CancelOnDrop(CancelToken);

impl Drop for CancelOnDrop {
	fn drop(&mut self) {
		self.0.cancel();
	}
}

/// The error a request ends with when its [`CancelToken`] was cancelled.
///
/// Distinct from a failure: the work was unwanted, not wrong. Find it with
/// [`Cancelled::is_cause_of`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cancelled;

impl Cancelled {
	/// Whether `error`, or anything in its chain of causes, is [`Cancelled`].
	///
	/// Only finds it while the error is still an error: a cause that was
	/// turned into text along the way, as `anyhow!("{e:#}")` does, is gone.
	#[must_use]
	pub fn is_cause_of(error: &anyhow::Error) -> bool {
		error.chain().any(<dyn std::error::Error>::is::<Cancelled>)
	}
}

impl fmt::Display for Cancelled {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str("the request was cancelled")
	}
}

impl std::error::Error for Cancelled {}

#[cfg(test)]
mod tests {
	use std::time::Duration;

	use anyhow::Context;

	use super::*;

	#[test]
	fn never_is_never_cancelled() {
		let token = CancelToken::never();
		token.cancel();
		assert!(!token.is_cancelled());
		assert!(token.check().is_ok());
	}

	#[test]
	fn a_cancelled_token_fails_its_check_with_cancelled() {
		let token = CancelToken::new();
		assert!(token.check().is_ok());
		token.clone().cancel();
		assert!(token.is_cancelled());

		let error = token.check().context("reading tile 3/1/2").unwrap_err();
		assert!(Cancelled::is_cause_of(&error));
		assert!(!Cancelled::is_cause_of(&anyhow::anyhow!("a real failure")));
	}

	#[test]
	fn a_deadline_cancels_once_it_passes() {
		assert!(CancelToken::with_deadline(Instant::now()).is_cancelled());
		assert!(!CancelToken::with_deadline(Instant::now() + Duration::from_secs(3600)).is_cancelled());
	}

	#[test]
	fn dropping_the_guard_cancels() {
		let token = CancelToken::new();
		drop(token.cancel_on_drop());
		assert!(token.is_cancelled());
	}

	#[tokio::test]
	async fn current_is_never_outside_a_scope() {
		let token = CancelToken::current();
		assert!(token.0.is_none());
	}

	#[tokio::test]
	async fn current_is_the_token_of_the_scope() {
		let token = CancelToken::new();
		token.cancel();
		let inner = token.clone().scope(async { CancelToken::current().is_cancelled() });
		assert!(inner.await);
	}

	/// Two requests polled side by side on one task each see their own token,
	/// across every point where either of them pauses.
	#[tokio::test]
	async fn concurrent_requests_on_one_task_see_their_own_token() {
		async fn request() -> Vec<bool> {
			let mut seen = vec![];
			for _ in 0..5 {
				seen.push(CancelToken::current().is_cancelled());
				tokio::task::yield_now().await;
			}
			seen
		}

		let cancelled = CancelToken::new();
		cancelled.cancel();
		let (a, b) = tokio::join!(cancelled.scope(request()), CancelToken::new().scope(request()));
		assert_eq!(a, vec![true; 5]);
		assert_eq!(b, vec![false; 5]);
	}

	/// The reason [`CancelToken::current`] must be captured before spawning.
	#[tokio::test]
	async fn spawn_blocking_does_not_inherit_the_token() {
		let token = CancelToken::new();
		token.cancel();
		let (inherited, captured) = token
			.scope(async {
				let captured = CancelToken::current();
				tokio::task::spawn_blocking(move || (CancelToken::current().is_cancelled(), captured.is_cancelled()))
					.await
					.unwrap()
			})
			.await;
		assert!(!inherited);
		assert!(captured);
	}

	#[tokio::test]
	async fn sync_scope_brings_the_token_back_on_the_blocking_pool() {
		let token = CancelToken::new();
		token.cancel();
		let captured = token;
		let seen = tokio::task::spawn_blocking(move || captured.sync_scope(|| CancelToken::current().is_cancelled()))
			.await
			.unwrap();
		assert!(seen);
	}
}
