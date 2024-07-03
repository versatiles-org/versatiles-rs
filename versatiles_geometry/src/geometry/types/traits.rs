use std::fmt::Debug;

pub trait SingleGeometryTrait<Multi>: Debug + Clone {
	fn area(&self) -> f64;
	fn into_multi(self) -> Multi;
}

pub trait MultiGeometryTrait: Debug + Clone {
	fn area(&self) -> f64;
}

pub trait VectorGeometryTrait<Item>: Debug + Clone {
	fn into_first_and_rest(self) -> (Item, Vec<Item>) {
		let mut iter = self.into_iter();
		let first = iter.next().unwrap();
		let rest = Vec::from_iter(iter);
		(first, rest)
	}
	fn into_iter(self) -> impl Iterator<Item = Item>;
	fn is_empty(&self) -> bool {
		self.len() == 0
	}
	fn len(&self) -> usize;
}
