use gdal::Dataset;

#[derive(Debug)]
pub struct Instance {
	dataset: Dataset,
	age: u32,
}

unsafe impl Sync for Instance {}

impl Instance {
	pub fn new(dataset: Dataset) -> Self {
		Self { dataset, age: 0 }
	}
	pub fn age(&self) -> u32 {
		self.age
	}
	pub fn cleanup(&mut self) {
		self.age = self.age.wrapping_add(1);
		self.dataset.flush_cache().unwrap();
	}
	pub fn dataset(&self) -> &Dataset {
		&self.dataset
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
	fn age_increments_monotonically() {
		let ds = mem_dataset(1, 1, 1);
		let mut inst = Instance::new(ds);
		assert_eq!(inst.age(), 0);
		inst.cleanup();
		assert_eq!(inst.age(), 1);
		inst.cleanup();
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
