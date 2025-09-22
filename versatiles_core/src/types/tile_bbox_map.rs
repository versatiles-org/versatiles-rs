use crate::{TileBBox, TileCoord, TileStream};
use anyhow::Result;
use std::fmt::Debug;
use versatiles_derive::context;

pub struct TileBBoxMap<I> {
	bbox: TileBBox,
	vec: Vec<I>,
}

impl<I> TileBBoxMap<I> {
	pub fn new_prefilled_with(bbox: TileBBox, item: I) -> Self
	where
		I: Clone,
	{
		let n = bbox.count_tiles() as usize;
		let mut vec = Vec::with_capacity(n);
		vec.resize(n, item);
		Self { bbox, vec }
	}

	pub fn new_default(bbox: TileBBox) -> Self
	where
		I: Clone + Default,
	{
		Self::new_prefilled_with(bbox, I::default())
	}

	pub fn len(&self) -> usize {
		self.vec.len()
	}

	pub fn is_empty(&self) -> bool {
		self.vec.is_empty()
	}

	pub fn bbox(&self) -> &TileBBox {
		&self.bbox
	}

	#[context("Failed to insert into TileBBoxMap at coord: {:?}", coord)]
	pub fn insert(&mut self, coord: TileCoord, item: I) -> Result<()> {
		let index = self.bbox.get_tile_index(&coord)?;
		self.vec[index as usize] = item;
		Ok(())
	}

	#[context("Failed to get from TileBBoxMap at coord: {:?}", coord)]
	pub fn get(&self, coord: &TileCoord) -> Result<&I> {
		let index = self.bbox.get_tile_index(coord)?;
		Ok(&self.vec[index as usize])
	}

	#[context("Failed to get mutably from TileBBoxMap at coord: {:?}", coord)]
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

	pub fn into_decreased_level(self) -> TileBBoxMap<Vec<(TileCoord, I)>>
	where
		I: Clone,
	{
		let bbox1 = self.bbox.as_level_decreased();
		self.vec.into_iter().enumerate().fold(
			TileBBoxMap::<Vec<(TileCoord, I)>>::new_default(bbox1),
			|mut container1, (i, item)| {
				let coord0 = self.bbox.get_coord_by_index(i as u64).unwrap();
				let coord1 = coord0.as_level(self.bbox.level - 1);
				container1.get_mut(&coord1).unwrap().push((coord0, item));
				container1
			},
		)
	}

	pub fn map<O: Clone>(self, f: impl FnMut(I) -> O) -> TileBBoxMap<O> {
		TileBBoxMap {
			bbox: self.bbox,
			vec: self.vec.into_iter().map(f).collect(),
		}
	}
}

impl<I> TileBBoxMap<Option<I>> {
	#[context("Failed to create TileBBoxMap from stream")]
	pub async fn from_stream(bbox: TileBBox, stream: TileStream<'_, I>) -> Result<Self>
	where
		I: Clone + Send,
	{
		let mut container = TileBBoxMap::<Option<I>>::new_default(bbox);
		let vec = stream.to_vec().await;
		for (coord, item) in vec {
			container.insert(coord, Some(item))?;
		}
		Ok(container)
	}

	#[context("Failed to create TileBBoxMap from iterator")]
	pub fn from_iter(bbox: TileBBox, iter: impl IntoIterator<Item = (TileCoord, I)>) -> Result<Self>
	where
		I: Clone,
	{
		let mut container = TileBBoxMap::<Option<I>>::new_prefilled_with(bbox, None);
		for (coord, item) in iter.into_iter() {
			container.insert(coord, Some(item))?;
		}
		Ok(container)
	}
}

impl<I: Debug> Debug for TileBBoxMap<I> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("TileBBoxMap").field("bbox", &self.bbox).finish()
	}
}

impl<I> std::iter::IntoIterator for TileBBoxMap<I> {
	type Item = (TileCoord, I);
	type IntoIter =
		std::iter::Map<std::iter::Enumerate<std::vec::IntoIter<I>>, Box<dyn Fn((usize, I)) -> (TileCoord, I) + Send>>;

	fn into_iter(self) -> Self::IntoIter {
		let bbox = self.bbox;
		let f = Box::new(move |(i, item)| (bbox.get_coord_by_index(i as u64).unwrap(), item));
		self.vec.into_iter().enumerate().map(f)
	}
}

impl<I: Clone> Clone for TileBBoxMap<I> {
	fn clone(&self) -> Self {
		TileBBoxMap {
			bbox: self.bbox,
			vec: self.vec.clone(),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn bb(l: u8, x0: u32, y0: u32, x1: u32, y1: u32) -> TileBBox {
		TileBBox::from_boundaries(l, x0, y0, x1, y1).unwrap()
	}
	fn c(l: u8, x: u32, y: u32) -> TileCoord {
		TileCoord::new(l, x, y).unwrap()
	}

	#[test]
	fn construct_prefilled_and_default() {
		let bbox = bb(4, 5, 6, 6, 7); // 2x2 tiles
		let m = TileBBoxMap::<u32>::new_prefilled_with(bbox, 9);
		assert_eq!(m.len(), 4);
		assert!(!m.is_empty());
		// default fills with Default::default()
		let m2 = TileBBoxMap::<Option<u8>>::new_default(bbox);
		assert_eq!(m2.len(), 4);
		for (_, v) in m2.iter() {
			assert_eq!(*v, None);
		}
	}

	#[test]
	fn insert_get_and_get_mut() -> Result<()> {
		let bbox = bb(3, 1, 2, 2, 3); // 2x2
		let mut m = TileBBoxMap::<i32>::new_prefilled_with(bbox, 0);
		m.insert(c(3, 1, 2), 10)?;
		m.insert(c(3, 2, 3), 20)?;
		assert_eq!(*m.get(&c(3, 1, 2))?, 10);
		assert_eq!(*m.get(&c(3, 2, 3))?, 20);
		*m.get_mut(&c(3, 2, 3))? = 30;
		assert_eq!(*m.get(&c(3, 2, 3))?, 30);
		Ok(())
	}

	#[test]
	fn iter_and_into_iter_yield_coords_in_bbox_order() {
		let bbox = bb(2, 0, 0, 1, 1); // coords: (0,0),(1,0),(0,1),(1,1)
		let mut m = TileBBoxMap::<u8>::new_prefilled_with(bbox, 0);
		// mark positions with 1..=4
		m.insert(c(2, 0, 0), 1).unwrap();
		m.insert(c(2, 1, 0), 2).unwrap();
		m.insert(c(2, 0, 1), 3).unwrap();
		m.insert(c(2, 1, 1), 4).unwrap();

		let via_iter: Vec<(TileCoord, u8)> = m.iter().map(|(tc, v)| (tc, *v)).collect();
		let via_into: Vec<(TileCoord, u8)> = m.clone().into_iter().collect();
		assert_eq!(via_iter, via_into);
		// Ensure ordering is x-fastest then y
		let coords: Vec<_> = via_iter.into_iter().map(|(tc, _)| (tc.x, tc.y)).collect();
		assert_eq!(coords, vec![(0, 0), (1, 0), (0, 1), (1, 1)]);
	}

	#[tokio::test]
	async fn from_stream_and_from_iter_fill_correct_slots() -> Result<()> {
		let bbox = bb(5, 10, 20, 11, 21); // 2x2
		let items = vec![(c(5, 10, 20), 'a'), (c(5, 11, 21), 'z')];
		// from_iter
		let m_it = TileBBoxMap::from_iter(bbox, items.clone().into_iter())?;
		assert_eq!(*m_it.get(&c(5, 10, 20))?, Some('a'));
		assert_eq!(*m_it.get(&c(5, 11, 21))?, Some('z'));
		assert_eq!(m_it.len(), 4);

		// from_stream
		let stream = TileStream::from_vec(items);
		let m_st = TileBBoxMap::from_stream(bbox, stream).await?;
		assert_eq!(*m_st.get(&c(5, 10, 20))?, Some('a'));
		assert_eq!(*m_st.get(&c(5, 11, 21))?, Some('z'));
		Ok(())
	}

	#[test]
	fn into_decreased_level_groups_four_children() {
		let bbox = bb(6, 8, 8, 9, 9); // 2x2 at level 6
		let mut m = TileBBoxMap::<u8>::new_prefilled_with(bbox, 0);
		m.insert(c(6, 8, 8), 1).unwrap();
		m.insert(c(6, 9, 8), 2).unwrap();
		m.insert(c(6, 8, 9), 3).unwrap();
		m.insert(c(6, 9, 9), 4).unwrap();
		let grouped = m.into_decreased_level(); // level 5, single parent (4 children)
		assert_eq!(grouped.len(), 1);
		let parent = c(5, 4, 4);
		let v = grouped.get(&parent).unwrap();
		// Expect four entries and child coords preserved
		assert_eq!(v.len(), 4);
		let coords: Vec<_> = v.iter().map(|(tc, _)| (tc.level, tc.x, tc.y)).collect();
		assert!(
			coords.contains(&(6, 8, 8))
				&& coords.contains(&(6, 9, 8))
				&& coords.contains(&(6, 8, 9))
				&& coords.contains(&(6, 9, 9))
		);
	}

	#[test]
	fn map_transforms_inner_items() {
		let bbox = bb(3, 0, 0, 1, 1);
		let m = TileBBoxMap::<u8>::new_prefilled_with(bbox, 5);
		let mapped = m.map(|v| v as u16 * 2);
		for (_, v) in mapped.iter() {
			assert_eq!(*v, 10);
		}
	}
}
