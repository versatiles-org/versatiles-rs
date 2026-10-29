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
//!   the others. Run the cache call through [`SharedWork::run`], which runs it
//!   under a token that is cancelled only once every request waiting for it
//!   is.
//!
//! Without a scope, [`CancelToken::current`] is a token that is never
//! cancelled, so code that checks costs nothing to callers that do not use
//! tokens, and bulk work such as a conversion is never stopped by one.

use std::{
	collections::HashMap,
	fmt,
	future::Future,
	hash::Hash,
	sync::{
		Arc, Mutex, Weak,
		atomic::{AtomicBool, AtomicU64, Ordering},
	},
	time::Instant,
};

use anyhow::{Result, anyhow};

tokio::task_local! {
	static CURRENT: CancelToken;
}

/// A request's permission to keep working. See the [module docs](self).
///
/// Clones share one state: cancelling any of them cancels all.
#[derive(Clone, Debug, Default)]
pub struct CancelToken(Option<Kind>);

#[derive(Clone, Debug)]
enum Kind {
	/// A request's own: cancelled by [`CancelToken::cancel`] or its deadline.
	Own(Arc<State>),
	/// Shared work's: cancelled once every request waiting for it is.
	All(Arc<Waiters>),
}

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
		Self(Some(Kind::Own(Arc::new(State {
			cancelled: AtomicBool::new(false),
			deadline,
		}))))
	}

	/// A token that is never cancelled. [`cancel`](Self::cancel) does nothing.
	#[must_use]
	pub fn never() -> Self {
		Self(None)
	}

	/// Cancels this token and every clone of it.
	///
	/// Does nothing to the token of shared work, which is cancelled by the
	/// requests waiting for it and by nothing else.
	pub fn cancel(&self) {
		if let Some(Kind::Own(state)) = &self.0 {
			state.cancelled.store(true, Ordering::Relaxed);
		}
	}

	/// Whether the token has been cancelled or its deadline has passed.
	#[must_use]
	pub fn is_cancelled(&self) -> bool {
		match &self.0 {
			None => false,
			Some(Kind::Own(state)) => {
				state.cancelled.load(Ordering::Relaxed) || state.deadline.is_some_and(|deadline| Instant::now() >= deadline)
			}
			Some(Kind::All(waiters)) => waiters.all_cancelled(),
		}
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

/// The requests waiting for one piece of shared work, by their tokens.
#[derive(Debug, Default)]
struct Waiters {
	tokens: Mutex<HashMap<u64, CancelToken>>,
	next_id: AtomicU64,
}

impl Waiters {
	/// Whether nobody waiting wants the work any more.
	///
	/// The tokens are copied out before they are asked: one may be the token of
	/// shared work in turn, with waiters of its own, and asking with the lock
	/// held would nest the locks.
	fn all_cancelled(&self) -> bool {
		let tokens: Vec<CancelToken> = self
			.tokens
			.lock()
			.expect("poisoned waiters")
			.values()
			.cloned()
			.collect();
		tokens.iter().all(CancelToken::is_cancelled)
	}
}

/// One request's interest in shared work, withdrawn when dropped.
struct Interest<'a, K: Hash + Eq + Clone> {
	work: &'a SharedWork<K>,
	key: K,
	waiters: Arc<Waiters>,
	id: u64,
}

impl<K: Hash + Eq + Clone> Drop for Interest<'_, K> {
	fn drop(&mut self) {
		self.waiters.tokens.lock().expect("poisoned waiters").remove(&self.id);
		// The last one out forgets the key. Nothing else holds the waiters
		// then, neither another request nor the work's own token, so no work
		// is running under them.
		if Arc::strong_count(&self.waiters) == 1 {
			let mut keys = self.work.keys.lock().expect("poisoned shared work");
			if keys
				.map
				.get(&self.key)
				.is_some_and(|weak| weak.as_ptr() == Arc::as_ptr(&self.waiters))
			{
				keys.map.remove(&self.key);
			}
		}
	}
}

/// Work that a cache shares between the requests asking for one key: cancelled
/// once every request waiting for it is, and only then. See the
/// [module docs](self).
///
/// Holds no results, only who is waiting for what; the cache it wraps keeps
/// the results and merges the requests.
#[derive(Debug)]
pub struct SharedWork<K> {
	keys: Mutex<Keys<K>>,
}

#[derive(Debug)]
struct Keys<K> {
	map: HashMap<K, Weak<Waiters>>,
	/// When to sweep out entries whose work is gone. An entry usually leaves
	/// with its last request, but not when something else still held the
	/// work's token then, and a long-running server must not collect those.
	prune_at: usize,
}

impl<K: Hash + Eq + Clone> Default for SharedWork<K> {
	fn default() -> Self {
		Self::new()
	}
}

impl<K: Hash + Eq + Clone> SharedWork<K> {
	/// No work registered yet.
	#[must_use]
	pub fn new() -> Self {
		Self {
			keys: Mutex::new(Keys {
				map: HashMap::new(),
				prune_at: 64,
			}),
		}
	}

	/// Runs `call`, a cache call that merges concurrent requests for `key`,
	/// under a token that is cancelled once every request waiting for `key`
	/// is. The current request is one of them until this returns.
	///
	/// A request that finds the work stopped for the others, while it still
	/// wants it, calls again: it may have arrived just as the last of them
	/// gave up. Its own cancellation ends it with [`Cancelled`].
	///
	/// # Errors
	/// Returns the error of `call`, or [`Cancelled`] if the current request is
	/// cancelled. `Cancelled` stays recognisable, though the cache's shared
	/// error is otherwise passed on as text.
	pub async fn run<T, Fut>(&self, key: K, mut call: impl FnMut() -> Fut) -> Result<T>
	where
		Fut: Future<Output = std::result::Result<T, Arc<anyhow::Error>>>,
	{
		// Each further try joins work no one but this request has cancelled,
		// so one is all it takes; the bound is only there to stop a loop.
		const TRIES: usize = 3;

		let mine = CancelToken::current();
		for _ in 0..TRIES {
			let (token, _interest) = self.join(&key, &mine);
			match token.scope(call()).await {
				Ok(value) => return Ok(value),
				Err(error) if Cancelled::is_cause_of(&error) => mine.check()?,
				Err(error) => return Err(anyhow!("{error:#}")),
			}
		}
		Err(anyhow!(
			"shared work stopped for others {TRIES} times while this request still wanted it"
		))
	}

	/// Registers `token` as waiting for `key`, and returns the token the work
	/// for `key` runs under.
	fn join(&self, key: &K, token: &CancelToken) -> (CancelToken, Interest<'_, K>) {
		let waiters = {
			let mut keys = self.keys.lock().expect("poisoned shared work");
			if let Some(waiters) = keys.map.get(key).and_then(Weak::upgrade) {
				waiters
			} else {
				if keys.map.len() >= keys.prune_at {
					keys.map.retain(|_, weak| weak.strong_count() > 0);
					keys.prune_at = keys.map.len().saturating_mul(2).max(64);
				}
				let waiters = Arc::new(Waiters::default());
				keys.map.insert(key.clone(), Arc::downgrade(&waiters));
				waiters
			}
		};
		let id = waiters.next_id.fetch_add(1, Ordering::Relaxed);
		waiters
			.tokens
			.lock()
			.expect("poisoned waiters")
			.insert(id, token.clone());
		let interest = Interest {
			work: self,
			key: key.clone(),
			waiters: Arc::clone(&waiters),
			id,
		};
		(CancelToken(Some(Kind::All(waiters))), interest)
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

	/// Requests waiting for the same work under their own tokens, with a way
	/// to see the token the work runs under and to let it finish.
	struct Overlap {
		work: Arc<SharedWork<u32>>,
		seen: Arc<Mutex<Vec<CancelToken>>>,
		release: Arc<tokio::sync::Notify>,
	}

	impl Overlap {
		fn new() -> Self {
			Self {
				work: Arc::new(SharedWork::new()),
				seen: Arc::default(),
				release: Arc::default(),
			}
		}

		/// A request with token `own`, for key 1, whose work records the token
		/// it runs under and then waits to be released.
		fn request(&self, own: CancelToken) -> tokio::task::JoinHandle<Result<()>> {
			let (work, seen, release) = (
				Arc::clone(&self.work),
				Arc::clone(&self.seen),
				Arc::clone(&self.release),
			);
			tokio::spawn(own.scope(async move {
				work
					.run(1, || async {
						seen.lock().unwrap().push(CancelToken::current());
						release.notified().await;
						Ok(())
					})
					.await
			}))
		}

		async fn until_seen(&self, count: usize) -> Vec<CancelToken> {
			loop {
				let seen = self.seen.lock().unwrap().clone();
				if seen.len() >= count {
					return seen;
				}
				tokio::task::yield_now().await;
			}
		}
	}

	#[tokio::test]
	async fn shared_work_is_cancelled_only_once_every_waiter_is() {
		let overlap = Overlap::new();
		let (a, b) = (CancelToken::new(), CancelToken::new());
		let requests = [overlap.request(a.clone()), overlap.request(b.clone())];
		let seen = overlap.until_seen(2).await;

		assert!(!seen[0].is_cancelled());
		a.cancel();
		assert!(!seen[0].is_cancelled(), "b still wants it");
		b.cancel();
		assert!(seen[0].is_cancelled() && seen[1].is_cancelled());

		overlap.release.notify_waiters();
		for request in requests {
			request.await.unwrap().unwrap();
		}
	}

	/// A request without a token, such as a conversion, never gives up, so
	/// nothing it waits for is ever cancelled.
	#[tokio::test]
	async fn a_waiter_without_a_token_keeps_shared_work_alive() {
		let overlap = Overlap::new();
		let a = CancelToken::new();
		let requests = [overlap.request(a.clone()), overlap.request(CancelToken::never())];
		let seen = overlap.until_seen(2).await;

		a.cancel();
		assert!(!seen[0].is_cancelled());

		overlap.release.notify_waiters();
		for request in requests {
			request.await.unwrap().unwrap();
		}
	}

	/// A dropped request, a closed tab with no explicit cancel, stops wanting
	/// the work as surely as a cancelled one.
	#[tokio::test]
	async fn a_dropped_waiter_withdraws_its_interest() {
		let overlap = Overlap::new();
		let (a, b) = (CancelToken::new(), CancelToken::new());
		let [first, second] = [overlap.request(a.clone()), overlap.request(b.clone())];
		let seen = overlap.until_seen(2).await;

		second.abort();
		assert!(second.await.unwrap_err().is_cancelled());
		a.cancel();
		assert!(seen[0].is_cancelled(), "nobody left who wants it");

		overlap.release.notify_waiters();
		first.await.unwrap().unwrap();
	}

	/// Shared work waiting for shared work, as an overview block waits for the
	/// blocks below it: the deeper work stops once the requests at the top do.
	#[test]
	fn shared_work_nests() {
		let (top, deep) = (SharedWork::new(), SharedWork::new());
		let own = CancelToken::new();
		let (top_token, _top_interest) = top.join(&1u32, &own);
		let (deep_token, _deep_interest) = deep.join(&2u32, &top_token);

		assert!(!deep_token.is_cancelled());
		own.cancel();
		assert!(deep_token.is_cancelled());
	}

	/// The late joiner: the work stopped because everyone else gave up, but this
	/// request still wants it, so it asks again rather than fail.
	#[tokio::test]
	async fn a_request_that_still_wants_stopped_work_asks_again() {
		let work = SharedWork::new();
		let mut calls = 0;
		let result = CancelToken::new()
			.scope(work.run(1u32, || {
				calls += 1;
				let first = calls == 1;
				async move {
					if first {
						Err(Arc::new(anyhow::Error::from(Cancelled)))
					} else {
						Ok("tile")
					}
				}
			}))
			.await;
		assert_eq!(result.unwrap(), "tile");
		assert_eq!(calls, 2);
	}

	#[tokio::test]
	async fn a_cancelled_request_ends_with_cancelled_and_other_errors_pass() {
		let work = SharedWork::new();
		let own = CancelToken::new();
		own.cancel();
		let error = own
			.scope(work.run(1u32, || async {
				Err::<(), _>(Arc::new(anyhow::Error::from(Cancelled)))
			}))
			.await
			.unwrap_err();
		assert!(Cancelled::is_cause_of(&error));

		let error = work
			.run(1u32, || async { Err::<(), _>(Arc::new(anyhow!("disk on fire"))) })
			.await
			.unwrap_err();
		assert_eq!(error.to_string(), "disk on fire");
	}

	#[tokio::test]
	async fn finished_work_forgets_its_key() {
		let work = SharedWork::new();
		for key in 0..1000u32 {
			work
				.run(key, || async { Ok::<_, Arc<anyhow::Error>>(()) })
				.await
				.unwrap();
		}
		assert!(work.keys.lock().unwrap().map.is_empty());
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
