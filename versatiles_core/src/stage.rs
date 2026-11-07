//! Which stage of a pipeline the current work belongs to, and what each stage
//! costs: CPU time, tiles delivered, and time spent waiting for it.
//!
//! A [`Stage`] is a set of counters. Work is attributed to the stage that is
//! [`current`](Stage::current), a task-local like
//! [`CancelToken`](crate::CancelToken):
//!
//! - [`Stage::in_poll`] makes a stage current while a stream or future of it is
//!   polled, and adds the CPU time of that poll to it, minus the time of stages
//!   polled inside it. Each stage pays for its own combinators that way, while
//!   the stages it reads from pay for theirs.
//! - [`spawn_blocking`] runs a closure on the blocking pool, adding its CPU
//!   time to the stage current where it was spawned. The parallel helpers of
//!   [`TileStream`](crate::TileStream) use it, so their callbacks are counted
//!   for the stage whose stream they belong to.
//! - [`spawn`] starts a task in which the current stage stays current, which a
//!   bare `tokio::spawn` would lose.
//!
//! Without a current stage, all of this costs nothing: no clock is read.

use std::{
	cell::Cell,
	future::Future,
	pin::Pin,
	sync::{
		Arc,
		atomic::{AtomicU64, Ordering},
	},
	task::{Context, Poll},
	time::Duration,
};

tokio::task_local! {
	static CURRENT: Arc<Stage>;
}

thread_local! {
	/// CPU time of stages polled inside the poll being measured on this
	/// thread, which that poll must not count as its own.
	static NESTED: Cell<Duration> = const { Cell::new(Duration::ZERO) };
}

/// The counters of one pipeline stage, such as one operation.
#[derive(Debug)]
pub struct Stage {
	name: String,
	input: Option<Arc<Stage>>,
	cpu_nanos: AtomicU64,
	tiles: AtomicU64,
	waited_nanos: AtomicU64,
}

/// What a stage has cost so far.
#[derive(Clone, Debug, PartialEq)]
pub struct StageStats {
	/// The stage's name, such as the operation's.
	pub name: String,
	/// The name of the stage it reads from, if it reads from exactly one.
	pub input: Option<String>,
	/// CPU time of its own work.
	pub cpu: Duration,
	/// Tiles it delivered.
	pub tiles: u64,
	/// Time spent waiting for its tiles, summed over everything that asked
	/// for them. Requests waiting side by side each count.
	pub waited: Duration,
}

impl Stage {
	/// A stage named `name` that reads from `input`.
	#[must_use]
	pub fn new(name: impl Into<String>, input: Option<Arc<Stage>>) -> Arc<Self> {
		Arc::new(Self {
			name: name.into(),
			input,
			cpu_nanos: AtomicU64::new(0),
			tiles: AtomicU64::new(0),
			waited_nanos: AtomicU64::new(0),
		})
	}

	/// The stage's name.
	#[must_use]
	pub fn name(&self) -> &str {
		&self.name
	}

	/// The stage whose work is being done, if any.
	#[must_use]
	pub fn current() -> Option<Arc<Self>> {
		CURRENT.try_with(Arc::clone).ok()
	}

	/// Runs one poll, `f`, of a stream or future of this stage: the stage is
	/// current during it, and the CPU time it takes, minus the time of stages
	/// polled inside it, is added to the stage.
	pub fn in_poll<R>(self: &Arc<Self>, f: impl FnOnce() -> R) -> R {
		let outer = NESTED.replace(Duration::ZERO);
		let started = thread_cpu_time();
		let result = CURRENT.sync_scope(Arc::clone(self), f);
		let spent = started.map_or(Duration::ZERO, |started| {
			thread_cpu_time().map_or(Duration::ZERO, |now| now.saturating_sub(started))
		});
		let nested = NESTED.replace(outer + spent);
		self.add_cpu(spent.saturating_sub(nested));
		result
	}

	/// `future`, with each of its polls run through [`in_poll`](Self::in_poll).
	pub fn instrument<F: Future>(self: &Arc<Self>, future: F) -> impl Future<Output = F::Output> {
		Instrumented {
			stage: Arc::clone(self),
			future: Box::pin(future),
		}
	}

	/// Counts one tile as delivered.
	pub fn add_tile(&self) {
		self.tiles.fetch_add(1, Ordering::Relaxed);
	}

	/// Adds time that something waited for this stage's tiles.
	pub fn add_wait(&self, waited: Duration) {
		self.waited_nanos.fetch_add(nanos(waited), Ordering::Relaxed);
	}

	fn add_cpu(&self, spent: Duration) {
		self.cpu_nanos.fetch_add(nanos(spent), Ordering::Relaxed);
	}

	/// The counters as they are now.
	#[must_use]
	pub fn stats(&self) -> StageStats {
		StageStats {
			name: self.name.clone(),
			input: self.input.as_ref().map(|input| input.name.clone()),
			cpu: Duration::from_nanos(self.cpu_nanos.load(Ordering::Relaxed)),
			tiles: self.tiles.load(Ordering::Relaxed),
			waited: Duration::from_nanos(self.waited_nanos.load(Ordering::Relaxed)),
		}
	}
}

/// A future whose polls are attributed to a stage.
struct Instrumented<F> {
	stage: Arc<Stage>,
	future: Pin<Box<F>>,
}

impl<F: Future> Future for Instrumented<F> {
	type Output = F::Output;

	fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<F::Output> {
		let this = self.get_mut();
		this.stage.in_poll(|| this.future.as_mut().poll(cx))
	}
}

/// Runs `f` on Tokio's blocking pool, as `tokio::task::spawn_blocking` does,
/// and adds its CPU time to the stage current here.
pub fn spawn_blocking<F, R>(f: F) -> tokio::task::JoinHandle<R>
where
	F: FnOnce() -> R + Send + 'static,
	R: Send + 'static,
{
	match Stage::current() {
		None => tokio::task::spawn_blocking(f),
		Some(stage) => tokio::task::spawn_blocking(move || {
			let started = thread_cpu_time();
			let result = CURRENT.sync_scope(Arc::clone(&stage), f);
			if let (Some(started), Some(now)) = (started, thread_cpu_time()) {
				stage.add_cpu(now.saturating_sub(started));
			}
			result
		}),
	}
}

/// Starts `future` as a task, as `tokio::spawn` does, with the stage current
/// here staying current in it.
pub fn spawn<F>(future: F) -> tokio::task::JoinHandle<F::Output>
where
	F: Future + Send + 'static,
	F::Output: Send + 'static,
{
	match Stage::current() {
		None => tokio::spawn(future),
		Some(stage) => tokio::spawn(CURRENT.scope(stage, future)),
	}
}

fn nanos(duration: Duration) -> u64 {
	u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

/// CPU time of the calling thread.
#[cfg(unix)]
fn thread_cpu_time() -> Option<Duration> {
	let mut time = libc::timespec { tv_sec: 0, tv_nsec: 0 };
	#[expect(
		unsafe_code,
		reason = "clock_gettime fills in the timespec it is given; see the SAFETY note"
	)]
	// SAFETY: `time` is a valid, writable `timespec` for the whole call.
	let result = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &raw mut time) };
	(result == 0).then(|| {
		Duration::new(
			u64::try_from(time.tv_sec).unwrap_or(0),
			u32::try_from(time.tv_nsec).unwrap_or(0),
		)
	})
}

/// Without a CPU clock per thread, work is not counted.
#[cfg(not(unix))]
fn thread_cpu_time() -> Option<Duration> {
	None
}

#[cfg(test)]
mod tests {
	use super::*;

	/// Spends `duration` of the thread's CPU time, however long that takes on
	/// a busy machine.
	#[cfg(unix)]
	fn burn(duration: Duration) {
		let started = thread_cpu_time().expect("a CPU clock");
		let mut x = 0u64;
		while thread_cpu_time().expect("a CPU clock").saturating_sub(started) < duration {
			for _ in 0..1000 {
				x = std::hint::black_box(x.wrapping_mul(31).wrapping_add(7));
			}
		}
	}

	#[test]
	fn a_stage_is_current_only_inside_its_polls() {
		let stage = Stage::new("a", None);
		assert!(Stage::current().is_none());
		let name = stage.in_poll(|| Stage::current().map(|current| current.name().to_string()));
		assert_eq!(name.as_deref(), Some("a"));
		assert!(Stage::current().is_none());
	}

	#[cfg(unix)]
	#[test]
	fn nested_stages_pay_for_their_own_time() {
		let outer = Stage::new("outer", None);
		let inner = Stage::new("inner", Some(Arc::clone(&outer)));
		outer.in_poll(|| {
			burn(Duration::from_millis(20));
			inner.in_poll(|| burn(Duration::from_millis(60)));
		});
		let (outer, inner) = (outer.stats().cpu, inner.stats().cpu);
		assert!(inner >= Duration::from_millis(50), "inner {inner:?}");
		assert!(
			outer >= Duration::from_millis(15) && outer < Duration::from_millis(50),
			"outer {outer:?}"
		);
	}

	#[cfg(unix)]
	#[tokio::test(flavor = "multi_thread")]
	async fn blocking_work_counts_for_the_stage_that_spawned_it() {
		let stage = Stage::new("blocking", None);
		let handle = stage.in_poll(|| spawn_blocking(|| burn(Duration::from_millis(40))));
		handle.await.expect("the task runs");
		assert!(stage.stats().cpu >= Duration::from_millis(30), "{:?}", stage.stats());
	}

	#[tokio::test]
	async fn a_spawned_task_keeps_the_stage() {
		let stage = Stage::new("task", None);
		let handle = stage.in_poll(|| spawn(async { Stage::current().map(|current| current.name().to_string()) }));
		assert_eq!(handle.await.expect("the task runs").as_deref(), Some("task"));
		let bare = tokio::spawn(async { Stage::current().is_none() });
		assert!(bare.await.expect("the task runs"));
	}

	#[tokio::test]
	async fn an_instrumented_future_runs_in_its_stage() {
		let stage = Stage::new("future", None);
		let name = stage
			.instrument(async {
				tokio::task::yield_now().await;
				Stage::current().map(|current| current.name().to_string())
			})
			.await;
		assert_eq!(name.as_deref(), Some("future"));
	}

	#[test]
	fn stats_name_the_input() {
		let input = Stage::new("read", None);
		let stage = Stage::new("shade", Some(input));
		stage.add_tile();
		stage.add_wait(Duration::from_millis(5));
		let stats = stage.stats();
		assert_eq!(stats.input.as_deref(), Some("read"));
		assert_eq!(stats.tiles, 1);
		assert_eq!(stats.waited, Duration::from_millis(5));
	}
}
