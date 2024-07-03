pub type Coordinates0 = [f64; 2];

pub type Coordinates1 = Vec<Coordinates0>;

pub type Coordinates2 = Vec<Coordinates1>;

pub type Coordinates3 = Vec<Coordinates2>;

pub trait Convertible
where
	Self: Copy,
{
	fn convert_coordinates0(value: [Self; 2]) -> Coordinates0
	where
		Self: Sized;
	fn convert_coordinates1(value: Vec<[Self; 2]>) -> Coordinates1
	where
		Self: Sized;
	fn convert_coordinates2(value: Vec<Vec<[Self; 2]>>) -> Coordinates2
	where
		Self: Sized;
	fn convert_coordinates3(value: Vec<Vec<Vec<[Self; 2]>>>) -> Coordinates3
	where
		Self: Sized;
}

macro_rules! impl_from_array {
	($($t:ty),*) => {$(
		impl Convertible for $t {
			fn convert_coordinates0(value: [$t; 2]) -> Coordinates0 {
				[value[0] as f64, value[1] as f64]
			}
			fn convert_coordinates1(value: Vec<[$t; 2]>) -> Coordinates1 {
				Vec::from_iter(value.into_iter().map(<$t>::convert_coordinates0))
			}
			fn convert_coordinates2(value: Vec<Vec<[$t; 2]>>) -> Coordinates2 {
				Vec::from_iter(value.into_iter().map(<$t>::convert_coordinates1))
			}
			fn convert_coordinates3(value: Vec<Vec<Vec<[$t; 2]>>>) -> Coordinates3 {
				Vec::from_iter(value.into_iter().map(<$t>::convert_coordinates2))
			}
		}
	)*}
}
impl_from_array!(i8, u8, i16, u16, i32, u32, i64, u64, f32);

impl Convertible for f64 {
	fn convert_coordinates0(value: [f64; 2]) -> Coordinates0 {
		value
	}
	fn convert_coordinates1(value: Vec<[f64; 2]>) -> Coordinates1 {
		value
	}
	fn convert_coordinates2(value: Vec<Vec<[f64; 2]>>) -> Coordinates2 {
		value
	}
	fn convert_coordinates3(value: Vec<Vec<Vec<[f64; 2]>>>) -> Coordinates3 {
		value
	}
}
