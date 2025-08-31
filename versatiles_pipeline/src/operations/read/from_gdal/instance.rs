use gdal::Dataset;
use std::cell::UnsafeCell;

#[derive(Debug)]
pub struct Instance {
	dataset: Dataset,
	locked: UnsafeCell<bool>,
}

unsafe impl Sync for Instance {}

impl Instance {
	pub fn new(dataset: Dataset) -> Self {
		Self {
			dataset,
			locked: UnsafeCell::new(false),
		}
	}
	pub fn lock(&self) {
		unsafe {
			*self.locked.get() = true;
		}
	}
	pub fn is_free(&self) -> bool {
		unsafe { !*self.locked.get() }
	}
	pub fn dataset(&self) -> &Dataset {
		&self.dataset
	}
}

impl Drop for Instance {
	fn drop(&mut self) {
		self.dataset.flush_cache().unwrap();
		unsafe {
			*self.locked.get() = false;
		}
	}
}

/*
/// Acquire a dataset guard from the pool. Uses `try_lock_owned` so selection and locking
/// is atomic, avoiding two tasks choosing the same handle and serializing.
async fn acquire_dataset(&self) -> Arc<Dataset> {
	const MAX_DATASETS: usize = 16;
	loop {
		// Try to lock any existing handle atomically
		if let Some(mut guard) = {
			let list = self.instances.lock().await;
			// Clone each Arc and attempt try_lock_owned; return the first that succeeds
			let mut found: Option<OwnedMutexGuard<Dataset>> = None;
			for ds in list.iter() {
				if let Ok(g) = ds.clone().try_lock_owned() {
					found = Some(g);
					break;
				}
			}
			found
		} {
			guard.flush_cache().unwrap();
			return guard;
		}

		// If we are at capacity, yield and retry
		let gdal_count = self.instances.lock().await.len();
		if gdal_count > MAX_DATASETS {
			warn!("managing {gdal_count} GDAL instances in pool");
		};

		// Grow the pool without holding the vec lock
		let ds = Dataset::open(&self.filename).expect("failed to open GDAL dataset");
		let ds = Arc::new(Mutex::new(ds));
		let mut list = self.instances.lock().await;
		list.push(ds);
		trace!("Growing GDAL dataset pool to {}", list.len());
		src.flush_cache()?;
	}
}

*/
