//! The block cache behind [`OverviewCore`](super::overview::OverviewCore).
//!
//! Two tiers with separate allocations, because they answer different
//! questions:
//!
//! - **pending** holds blocks that have been produced and are owed to a
//!   consumer the traversal has already scheduled. Nothing evicts them. This
//!   is what makes a depth-first pass compute every block exactly once — not a
//!   cache policy that usually keeps the right thing, but a reservation that
//!   cannot lose it.
//! - **reuse** holds blocks kept in case someone asks again: one LRU per zoom
//!   level, each with an equal share of the budget.
//!
//! Equal *byte* shares are what makes low zoom levels effectively permanent
//! without any explicit rule saying so. A level holds 4^L blocks, so an equal
//! share caches a far larger fraction of a sparse low-zoom level than of a
//! dense deep one — and low zoom is exactly where rebuilding costs the most,
//! since a block at level L is composed from 4^(base-L) source tiles.
//!
//! The two allocations are separate on purpose: no amount of pressure in the
//! reuse tier can evict something the traversal is still owed.

use std::{
	collections::{HashMap, VecDeque},
	sync::{
		Arc, Mutex,
		atomic::{AtomicBool, AtomicUsize, Ordering},
	},
};

use moka::future::Cache;
use versatiles_core::TileCoord;

use super::overview::{ScaledBlock, estimate_entry_bytes};

/// Memory the cache may use, unless [`CACHE_BUDGET_ENV`] says otherwise.
///
/// The figure the previous implementation merely warned about once it was
/// passed; here it is an actual bound.
const DEFAULT_BUDGET_MB: usize = 2048;

/// Overrides [`DEFAULT_BUDGET_MB`], in megabytes.
const CACHE_BUDGET_ENV: &str = "VERSATILES_OVERVIEW_CACHE_MB";

/// Share of the budget reserved for blocks awaiting their consumer.
///
/// A depth-first traversal holds at most four blocks per level along the path
/// it is currently walking, which is well under half the default budget for
/// any realistic pyramid.
const PENDING_SHARE: usize = 2;

/// Blocks produced but not yet consumed, with the order they arrived in.
#[derive(Default)]
struct Pending {
	blocks: HashMap<TileCoord, ScaledBlock>,
	order: VecDeque<TileCoord>,
	bytes: usize,
}

/// The two-tier store of scaled blocks.
pub struct BlockCache {
	pending: Mutex<Pending>,
	pending_limit: usize,
	peak_pending_bytes: AtomicUsize,
	/// Set once the pending tier has had to shed a block, so the warning that
	/// the guarantee no longer holds is printed once rather than per block.
	demotion_warned: AtomicBool,

	/// One LRU per zoom level, `0..=level_base`.
	reuse: Vec<Cache<TileCoord, ScaledBlock>>,

	/// How many blocks were built from scratch. On a depth-first pass this is
	/// the number of blocks in the pyramid; anything more means work was redone.
	blocks_computed: AtomicUsize,
}

impl BlockCache {
	/// Build a cache sized for a pyramid whose base is `level_base`.
	pub fn new(level_base: u8) -> Self {
		Self::with_budget(level_base, budget_bytes())
	}

	/// [`new`](Self::new) with the budget given rather than read from the
	/// environment, so a test can exercise the limits without allocating them.
	pub(crate) fn with_budget(level_base: u8, budget: usize) -> Self {
		let pending_limit = budget / PENDING_SHARE;

		// Level 0 is never cached — nothing is composed from it — but keeping
		// the vector indexable by level is worth one unused entry.
		let levels = usize::from(level_base) + 1;
		let per_level = u64::try_from((budget - pending_limit) / levels)
			.unwrap_or(u64::MAX)
			.max(1);

		let reuse = (0..levels)
			.map(|_| {
				Cache::builder()
					.max_capacity(per_level)
					.weigher(|_key: &TileCoord, block: &ScaledBlock| -> u32 {
						u32::try_from(estimate_entry_bytes(block)).unwrap_or(u32::MAX)
					})
					.build()
			})
			.collect();

		log::debug!(
			"overview cache: {} MiB budget, {} MiB reserved for blocks awaiting their consumer, {} MiB per zoom level",
			budget >> 20,
			pending_limit >> 20,
			per_level >> 20
		);

		Self {
			pending: Mutex::new(Pending::default()),
			pending_limit,
			peak_pending_bytes: AtomicUsize::new(0),
			demotion_warned: AtomicBool::new(false),
			reuse,
			blocks_computed: AtomicUsize::new(0),
		}
	}

	/// Record a block that the level below is expected to consume.
	///
	/// It stays put until [`consume`](Self::consume) takes it, unless the
	/// pending tier overflows — see [`Self::shed`].
	pub async fn produce(&self, key: TileCoord, block: ScaledBlock) {
		let bytes = estimate_entry_bytes(&block);

		let shed = {
			let mut pending = self.pending.lock().expect("poisoned pending lock");
			if pending.blocks.insert(key, block).is_none() {
				pending.order.push_back(key);
				pending.bytes += bytes;
			}
			self.peak_pending_bytes.fetch_max(pending.bytes, Ordering::Relaxed);
			Self::over_limit(&mut pending, self.pending_limit)
		};

		self.shed(shed).await;
	}

	/// Hand the block at `key` to the consumer that is about to build the level
	/// below it, moving it out of the reservation and into the reuse tier.
	///
	/// The traversal schedules exactly one consumer per block, so a pending
	/// block is owed to whoever calls this — after which it is only worth
	/// keeping in case a later request happens to want it again.
	pub async fn consume(&self, key: TileCoord) -> Option<ScaledBlock> {
		let taken = {
			let mut pending = self.pending.lock().expect("poisoned pending lock");
			pending.blocks.remove(&key).inspect(|block| {
				pending.order.retain(|k| k != &key);
				pending.bytes = pending.bytes.saturating_sub(estimate_entry_bytes(block));
			})
		};

		match taken {
			Some(block) => {
				self.store(key, block.clone()).await;
				Some(block)
			}
			None => self.reuse_tier(key.level)?.get(&key).await,
		}
	}

	/// The block at `key` if either tier holds it, leaving both untouched.
	pub async fn peek(&self, key: TileCoord) -> Option<ScaledBlock> {
		if let Some(block) = self
			.pending
			.lock()
			.expect("poisoned pending lock")
			.blocks
			.get(&key)
			.cloned()
		{
			return Some(block);
		}
		self.reuse_tier(key.level)?.get(&key).await
	}

	/// The block at `key`, running `init` if neither tier has it.
	///
	/// Concurrent callers asking for the same missing block share one `init`
	/// rather than each rebuilding the whole subtree underneath it.
	pub async fn get_or_compute<F, E>(&self, key: TileCoord, init: F) -> Result<ScaledBlock, Arc<E>>
	where
		F: std::future::Future<Output = Result<ScaledBlock, E>>,
		E: Send + Sync + 'static,
	{
		if let Some(block) = self.peek(key).await {
			return Ok(block);
		}

		let Some(tier) = self.reuse_tier(key.level) else {
			// A level with no tier of its own is still answerable, just not
			// cacheable; `init` is the whole answer.
			return init.await.map_err(Arc::new);
		};

		tier
			.try_get_with(key, async {
				self.blocks_computed.fetch_add(1, Ordering::Relaxed);
				init.await
			})
			.await
	}

	/// Put a block in the reuse tier for its level.
	async fn store(&self, key: TileCoord, block: ScaledBlock) {
		if let Some(tier) = self.reuse_tier(key.level) {
			tier.insert(key, block).await;
		}
	}

	fn reuse_tier(&self, level: u8) -> Option<&Cache<TileCoord, ScaledBlock>> {
		self.reuse.get(usize::from(level))
	}

	/// Oldest-first, the pending entries that no longer fit the reservation.
	fn over_limit(pending: &mut Pending, limit: usize) -> Vec<(TileCoord, ScaledBlock)> {
		let mut shed = Vec::new();
		while pending.bytes > limit {
			let Some(key) = pending.order.pop_front() else { break };
			let Some(block) = pending.blocks.remove(&key) else {
				continue;
			};
			pending.bytes = pending.bytes.saturating_sub(estimate_entry_bytes(&block));
			shed.push((key, block));
		}
		shed
	}

	/// Move blocks out of the reservation because it is full.
	///
	/// They are not dropped — the reuse tier may still have them when their
	/// consumer arrives — but they are no longer guaranteed to be there, so
	/// this is the point where "computed exactly once" stops being a promise.
	/// Worth saying out loud, once, with the way to fix it.
	async fn shed(&self, blocks: Vec<(TileCoord, ScaledBlock)>) {
		if blocks.is_empty() {
			return;
		}

		if !self.demotion_warned.swap(true, Ordering::Relaxed) {
			log::warn!(
				"overview: the {} MiB reserved for blocks awaiting their consumer is full, so some may have to be \
				 rebuilt. Raise {CACHE_BUDGET_ENV} (megabytes, currently {}) to avoid the extra work.",
				self.pending_limit >> 20,
				budget_bytes() >> 20
			);
		}

		for (key, block) in blocks {
			self.store(key, block).await;
		}
	}

	/// How many blocks both tiers hold together.
	///
	/// moka applies inserts and evictions from a queue, so its half of the
	/// count is only exact once that queue has been drained.
	#[cfg(test)]
	pub async fn entry_count(&self) -> u64 {
		let mut count = self.pending.lock().expect("poisoned pending lock").blocks.len() as u64;
		for tier in &self.reuse {
			tier.run_pending_tasks().await;
			count += tier.entry_count();
		}
		count
	}

	/// How many bytes both tiers hold together.
	#[cfg(test)]
	pub async fn total_bytes(&self) -> u64 {
		let mut bytes = self.pending.lock().expect("poisoned pending lock").bytes as u64;
		for tier in &self.reuse {
			tier.run_pending_tasks().await;
			bytes += tier.weighted_size();
		}
		bytes
	}

	/// Blocks built from scratch, and the high-water mark of the reservation.
	pub fn stats(&self) -> (usize, usize) {
		(
			self.blocks_computed.load(Ordering::Relaxed),
			self.peak_pending_bytes.load(Ordering::Relaxed),
		)
	}
}

impl std::fmt::Debug for BlockCache {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let (computed, peak) = self.stats();
		f.debug_struct("BlockCache")
			.field("pending_limit", &self.pending_limit)
			.field("blocks_computed", &computed)
			.field("peak_pending_bytes", &peak)
			.finish()
	}
}

/// The configured memory budget, in bytes.
fn budget_bytes() -> usize {
	parse_budget(std::env::var(CACHE_BUDGET_ENV).ok().as_deref())
}

/// [`budget_bytes`] without the environment, so the parsing can be tested
/// without a process-wide variable that every other test would also see.
fn parse_budget(raw: Option<&str>) -> usize {
	let mb = match raw {
		None => DEFAULT_BUDGET_MB,
		Some(raw) => match raw.trim().parse::<usize>() {
			Ok(mb) if mb > 0 => mb,
			_ => {
				log::warn!(
					"{CACHE_BUDGET_ENV} is \"{raw}\", which is not a positive number of megabytes; using {DEFAULT_BUDGET_MB}"
				);
				DEFAULT_BUDGET_MB
			}
		},
	};
	mb * 1024 * 1024
}

#[cfg(test)]
mod tests {
	use versatiles_core::Blob;

	use super::*;

	/// Small enough that the limits can be reached without allocating for real.
	const TEST_BUDGET: usize = 64 * 1024;

	fn cache(level_base: u8) -> BlockCache {
		BlockCache::with_budget(level_base, TEST_BUDGET)
	}

	fn block(coord: TileCoord, bytes: usize) -> ScaledBlock {
		Arc::new(vec![(coord, Some(Blob::from(vec![0u8; bytes])))])
	}

	#[tokio::test]
	async fn a_produced_block_is_handed_to_its_consumer() {
		let cache = cache(6);
		let key = TileCoord::new(6, 0, 0).unwrap();

		cache.produce(key, block(key, 100)).await;
		assert_eq!(cache.entry_count().await, 1);

		assert!(cache.consume(key).await.is_some(), "the consumer should get the block");
	}

	/// Consuming moves a block to the reuse tier rather than dropping it.
	#[tokio::test]
	async fn a_consumed_block_stays_available() {
		let cache = cache(6);
		let key = TileCoord::new(6, 0, 0).unwrap();

		cache.produce(key, block(key, 100)).await;
		cache.consume(key).await.expect("first consumer");

		assert!(
			cache.consume(key).await.is_some(),
			"a second reader should still find the block"
		);
		assert!(cache.peek(key).await.is_some(), "and so should a peek");
	}

	#[tokio::test]
	async fn an_unknown_block_is_absent_from_both_tiers() {
		let cache = cache(6);
		let key = TileCoord::new(6, 3, 4).unwrap();

		assert!(cache.peek(key).await.is_none());
		assert!(cache.consume(key).await.is_none());
	}

	/// Nothing in the reuse tier can push a block out of the reservation.
	///
	/// The two have separate allocations precisely so that filling one cannot
	/// cost the other a block the traversal is still owed.
	#[tokio::test]
	async fn reuse_pressure_does_not_touch_the_reservation() {
		let cache = cache(6);
		let owed = TileCoord::new(6, 0, 0).unwrap();
		cache.produce(owed, block(owed, 64)).await;

		// Comfortably more than one level's share of the budget.
		let each = TEST_BUDGET / 8;
		for x in 1..40u32 {
			let key = TileCoord::new(6, x, 0).unwrap();
			cache.store(key, block(key, each)).await;
		}

		assert!(
			cache.consume(owed).await.is_some(),
			"the reservation lost a block to reuse-tier pressure"
		);
	}

	#[tokio::test]
	async fn get_or_compute_runs_init_once_and_counts_it() {
		let cache = cache(6);
		let key = TileCoord::new(6, 1, 1).unwrap();

		let first: Result<_, Arc<anyhow::Error>> = cache
			.get_or_compute(key, async { Ok::<_, anyhow::Error>(block(key, 10)) })
			.await;
		assert!(first.is_ok());

		let second: Result<_, Arc<anyhow::Error>> = cache
			.get_or_compute(key, async { panic!("must not be computed twice") })
			.await;
		assert!(second.is_ok());

		assert_eq!(cache.stats().0, 1, "exactly one block should have been built");
	}

	#[tokio::test]
	async fn an_overflowing_reservation_sheds_its_oldest_blocks() {
		let cache = cache(6);
		let limit = cache.pending_limit;

		let each = limit / 4;
		for x in 0..10u32 {
			let key = TileCoord::new(6, x, 0).unwrap();
			cache.produce(key, block(key, each)).await;
		}

		let pending_bytes = cache.pending.lock().unwrap().bytes;
		assert!(
			pending_bytes <= limit,
			"the reservation grew past its limit: {pending_bytes} > {limit}"
		);
		assert!(
			cache.demotion_warned.load(Ordering::Relaxed),
			"shedding blocks should have been reported"
		);
	}

	/// A shed block is demoted, not dropped.
	#[tokio::test]
	async fn shedding_keeps_the_blocks_it_sheds() {
		let cache = cache(6);
		let first = TileCoord::new(6, 0, 0).unwrap();
		let each = cache.pending_limit / 2;

		cache.produce(first, block(first, each)).await;
		for x in 1..5u32 {
			let key = TileCoord::new(6, x, 0).unwrap();
			cache.produce(key, block(key, each)).await;
		}

		assert!(
			cache.peek(first).await.is_some(),
			"the oldest block was dropped rather than moved to the reuse tier"
		);
	}

	#[test]
	fn the_budget_is_read_in_megabytes() {
		assert_eq!(parse_budget(Some("512")), 512 * 1024 * 1024);
		assert_eq!(parse_budget(Some("  64  ")), 64 * 1024 * 1024);
	}

	/// Anything unusable falls back to the default rather than to nothing.
	///
	/// A cache sized zero would still be correct — every block would just be
	/// rebuilt — which is exactly the kind of silent collapse worth avoiding.
	#[test]
	fn a_nonsense_budget_falls_back_to_the_default() {
		let default = DEFAULT_BUDGET_MB * 1024 * 1024;
		assert_eq!(parse_budget(None), default);
		assert_eq!(parse_budget(Some("")), default);
		assert_eq!(parse_budget(Some("0")), default);
		assert_eq!(parse_budget(Some("lots")), default);
		assert_eq!(parse_budget(Some("-1")), default);
		assert_eq!(parse_budget(Some("1.5")), default);
	}
}
