use crate::{TileBBox, TileCoord, TileStream};
use anyhow::Result;
use std::fmt::Debug;
use versatiles_derive::context;

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

	pub fn len(&self) -> usize {
		self.vec.len()
	}

	pub fn is_empty(&self) -> bool {
		self.vec.is_empty()
	}

	#[context("Failed to create TileBBoxContainer from stream")]
	pub async fn from_stream<E: Clone + Send>(
		bbox: TileBBox,
		stream: TileStream<'_, E>,
	) -> Result<TileBBoxContainer<Option<E>>> {
		let mut container = TileBBoxContainer::<Option<E>>::new_default(bbox);
		let vec = stream.to_vec().await;
		for (coord, item) in vec {
			container.insert(coord, Some(item))?;
		}
		Ok(container)
	}

	#[context("Failed to create TileBBoxContainer from iterator")]
	pub fn from_iter<E: Clone>(
		bbox: TileBBox,
		iter: impl Iterator<Item = (TileCoord, E)>,
	) -> Result<TileBBoxContainer<Option<E>>> {
		let mut container = TileBBoxContainer::<Option<E>>::new_prefilled_with(bbox, None);
		for (coord, item) in iter {
			container.insert(coord, Some(item))?;
		}
		Ok(container)
	}

	pub fn bbox(&self) -> &TileBBox {
		&self.bbox
	}

	#[context("Failed to insert into TileBBoxContainer at coord: {:?}", coord)]
	pub fn insert(&mut self, coord: TileCoord, item: I) -> Result<()> {
		let index = self.bbox.get_tile_index(&coord)?;
		self.vec[index as usize] = item;
		Ok(())
	}

	#[context("Failed to get from TileBBoxContainer at coord: {:?}", coord)]
	pub fn get(&self, coord: &TileCoord) -> Result<&I> {
		let index = self.bbox.get_tile_index(coord)?;
		Ok(&self.vec[index as usize])
	}

	#[context("Failed to get mutably from TileBBoxContainer at coord: {:?}", coord)]
	pub fn get_mut(&mut self, coord: &TileCoord) -> Result<&mut I> {
		let index = self.bbox.get_tile_index(coord)?;
		Ok(&mut self.vec[index as usize])
	}

	pub fn iter(&self) -> impl Iterator<Item = (TileCoord, &I)> {
		self
			.vec
			.iter()
			.enumerate()
			.map(move |(i, item)| (self.bbox.get_coord_by_index(i as u64).unwrap(), item))
	}

	pub fn into_decreased_level(self) -> TileBBoxContainer<Vec<(TileCoord, I)>> {
		let bbox1 = self.bbox.as_level_decreased();
		self.vec.into_iter().enumerate().fold(
			TileBBoxContainer::<Vec<(TileCoord, I)>>::new_default(bbox1),
			|mut container1, (i, item)| {
				let coord0 = self.bbox.get_coord_by_index(i as u64).unwrap();
				let coord1 = coord0.as_level(self.bbox.level - 1);
				container1.get_mut(&coord1).unwrap().push((coord0, item));
				container1
			},
		)
	}

	pub fn map<O>(self, f: impl FnMut(I) -> O) -> TileBBoxContainer<O> {
		TileBBoxContainer {
			bbox: self.bbox,
			vec: self.vec.into_iter().map(f).collect(),
		}
	}
}

impl<I: Debug> Debug for TileBBoxContainer<I> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("TileBBoxContainer").field("bbox", &self.bbox).finish()
	}
}

impl<I: Clone> std::iter::IntoIterator for TileBBoxContainer<I> {
	type Item = (TileCoord, I);
	type IntoIter =
		std::iter::Map<std::iter::Enumerate<std::vec::IntoIter<I>>, Box<dyn Fn((usize, I)) -> (TileCoord, I) + Send>>;

	fn into_iter(self) -> Self::IntoIter {
		let bbox = self.bbox;
		let f = Box::new(move |(i, item)| (bbox.get_coord_by_index(i as u64).unwrap(), item));
		self.vec.into_iter().enumerate().map(f)
	}
}
