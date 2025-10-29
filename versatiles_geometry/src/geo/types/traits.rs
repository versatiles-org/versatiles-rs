use anyhow::Result;
use std::fmt::Debug;
use versatiles_core::json::JsonValue;

pub trait GeometryTrait: Debug + Clone {
	fn area(&self) -> f64;
	fn verify(&self) -> Result<()>;
	fn to_coord_json(&self, precision: Option<u8>) -> JsonValue;
}

pub trait SingleGeometryTrait<Multi>: Debug + Clone {
	fn into_multi(self) -> Multi;
}

pub trait CompositeGeometryTrait<Item>: Debug + Clone {
	fn new() -> Self;

	fn as_vec(&self) -> &Vec<Item>;
	fn as_mut_vec(&mut self) -> &mut Vec<Item>;
	fn into_inner(self) -> Vec<Item>;

	fn into_iter(self) -> impl Iterator<Item = Item> {
		self.into_inner().into_iter()
	}

	fn into_first_and_rest(self) -> Option<(Item, Vec<Item>)> {
		let mut iter = self.into_iter();
		iter.next().map(|first| (first, iter.collect()))
	}

	fn is_empty(&self) -> bool {
		self.as_vec().is_empty()
	}
	fn len(&self) -> usize {
		self.as_vec().len()
	}
	fn push(&mut self, item: Item) {
		self.as_mut_vec().push(item);
	}
	fn pop(&mut self) -> Option<Item> {
		self.as_mut_vec().pop()
	}
	fn first(&self) -> Option<&Item> {
		self.as_vec().first()
	}
	fn last(&self) -> Option<&Item> {
		self.as_vec().last()
	}
	fn first_mut(&mut self) -> Option<&mut Item> {
		self.as_mut_vec().first_mut()
	}
	fn last_mut(&mut self) -> Option<&mut Item> {
		self.as_mut_vec().last_mut()
	}
}
