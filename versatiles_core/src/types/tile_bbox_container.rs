use std::fmt::Debug;

use crate::{TileBBox, TileCoord3, TileStream};
use anyhow::Result;
use futures::StreamExt;

pub struct TileBBoxContainer<I> {
	bbox: TileBBox,
	vec: Vec<I>,
}

impl<I: Clone + Default> TileBBoxContainer<I> {
	pub fn new_prefilled_with(bbox: TileBBox, item: I) -> Self {
		let n = bbox.count_tiles() as usize;
		let mut vec = Vec::with_capacity(n);
		vec.resize(n, item);
		Self { bbox, vec }
	}

	pub fn new_default(bbox: TileBBox) -> Self {
		Self::new_prefilled_with(bbox, I::default())
	}

	pub async fn from_stream<E: Clone>(
		bbox: TileBBox,
		mut stream: TileStream<'_, E>,
	) -> Result<TileBBoxContainer<Option<E>>> {
		let mut container = TileBBoxContainer::<Option<E>>::new_prefilled_with(bbox, None);
		while let Some((coord, item)) = stream.stream.next().await {
			container.set(coord, Some(item))?;
		}
		Ok(container)
	}

	pub fn from_iter<E: Clone>(
		bbox: TileBBox,
		iter: impl Iterator<Item = (TileCoord3, E)>,
	) -> Result<TileBBoxContainer<Option<E>>> {
		let mut container = TileBBoxContainer::<Option<E>>::new_prefilled_with(bbox, None);
		for (coord, item) in iter {
			container.set(coord, Some(item))?;
		}
		Ok(container)
	}

	pub fn bbox(&self) -> &TileBBox {
		&self.bbox
	}

	pub fn set(&mut self, coord: TileCoord3, item: I) -> Result<()> {
		let index = self.bbox.get_tile_index3(&coord)?;
		self.vec[index as usize] = item;
		Ok(())
	}

	pub fn get(&self, coord: &TileCoord3) -> Result<&I> {
		let index = self.bbox.get_tile_index3(coord)?;
		Ok(&self.vec[index as usize])
	}

	pub fn get_mut(&mut self, coord: &TileCoord3) -> Result<&mut I> {
		let index = self.bbox.get_tile_index3(coord)?;
		Ok(&mut self.vec[index as usize])
	}

	pub fn iter(&self) -> impl Iterator<Item = (TileCoord3, &I)> {
		self
			.vec
			.iter()
			.enumerate()
			.map(move |(i, item)| (self.bbox.get_coord3_by_index(i as u64).unwrap(), item))
	}
}

impl<I: Debug> Debug for TileBBoxContainer<I> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("TileBBoxContainer").field("bbox", &self.bbox).finish()
	}
}

impl<I: Clone> std::iter::IntoIterator for TileBBoxContainer<I> {
	type Item = (TileCoord3, I);
	type IntoIter =
		std::iter::Map<std::iter::Enumerate<std::vec::IntoIter<I>>, Box<dyn Fn((usize, I)) -> (TileCoord3, I) + Send>>;

	fn into_iter(self) -> Self::IntoIter {
		let bbox = self.bbox;
		let f = Box::new(move |(i, item)| (bbox.get_coord3_by_index(i as u64).unwrap(), item));
		self.vec.into_iter().enumerate().map(f)
	}
}
