use gdal::Dataset;
use std::cell::UnsafeCell;

#[derive(Debug)]
pub struct Instance {
	dataset: UnsafeCell<Dataset>,
	locked: UnsafeCell<bool>,
	age: UnsafeCell<u32>,
}

unsafe impl Sync for Instance {}

impl Instance {
	pub fn new(dataset: Dataset) -> Self {
		Self {
			dataset: UnsafeCell::new(dataset),
			locked: UnsafeCell::new(false),
			age: UnsafeCell::new(0),
		}
	}
	pub fn try_lock(&self) -> bool {
		unsafe {
			if !*self.locked.get() {
				*self.locked.get() = true;
				*self.age.get() += 1;
				return true;
			}
		}
		false
	}
	pub fn age(&self) -> u32 {
		unsafe { *self.age.get() }
	}
	pub fn free(&self) {
		unsafe {
			self.dataset.get().as_mut().unwrap().flush_cache().unwrap();
			*self.locked.get() = false;
		}
	}
	pub fn dataset(&self) -> &Dataset {
		unsafe { &*self.dataset.get() }
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use gdal::DriverManager;

	fn mem_dataset(w: usize, h: usize, bands: usize) -> Dataset {
		let driver = DriverManager::get_driver_by_name("MEM").expect("MEM driver");
		driver
			.create_with_band_type::<u8, _>("", w, h, bands)
			.expect("create mem dataset")
	}

	#[test]
	fn try_lock_free_cycle() {
		let ds = mem_dataset(2, 2, 1);
		let inst = Instance::new(ds);

		// First lock should succeed
		assert!(inst.try_lock());
		// Second lock should fail while locked
		assert!(!inst.try_lock());

		// Free should unlock and flush without panicking
		inst.free();
		// Now lock should succeed again
		assert!(inst.try_lock());
	}

	#[test]
	fn age_increments_monotonically() {
		let ds = mem_dataset(1, 1, 1);
		let inst = Instance::new(ds);
		assert_eq!(inst.age(), 0);
		assert!(inst.try_lock());
		assert_eq!(inst.age(), 1);
		assert!(!inst.try_lock());
		inst.free();
		assert!(inst.try_lock());
		assert_eq!(inst.age(), 2);
	}

	#[test]
	fn dataset_access_returns_same_dataset() {
		let ds = mem_dataset(4, 3, 2);
		let inst = Instance::new(ds);
		let dref = inst.dataset();
		let (w, h) = dref.raster_size();
		assert_eq!((w as isize, h as isize), (4, 3));
		assert_eq!(dref.raster_count(), 2);
	}
}
