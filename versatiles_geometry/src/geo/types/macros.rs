#[macro_export]
/// Automatically implements `From` conversions for vector- and array-like collections into geometry container types.
///
/// This macro takes pairs of types `($t, $i)` where:
/// - `$t` is the target container type (e.g., `LineStringGeometry`, `PolygonGeometry`).
/// - `$i` is the element type contained within (e.g., `Coordinates`, `LineStringGeometry`).
///
/// The generated implementations allow easy construction of these container types from vectors or slices of elements that can be converted into the contained type.
macro_rules! impl_from_array {
	($($t:ty,$i:ty),*) => {$(
		impl<T> From<Vec<T>> for $t
		where
			$i: From<T>,
		{
			fn from(value: Vec<T>) -> Self {
				Self(value.into_iter().map(<$i>::from).collect())
			}
		}

		impl<'a, T> From<&'a Vec<T>> for $t
		where
			$i: From<&'a T>,
		{
			fn from(value: &'a Vec<T>) -> Self {
				Self(value.iter().map(<$i>::from).collect())
			}
		}

		impl<'a, T> From<&'a [T]> for $t
		where
			$i: From<&'a T>,
		{
			fn from(value: &'a [T]) -> Self {
				Self(value.iter().map(<$i>::from).collect())
			}
		}

		impl<'a, T, const N: usize> From<&'a [T; N]> for $t
		where
			$i: From<&'a T>,
		{
			fn from(value: &'a [T; N]) -> Self {
				Self(value.iter().map(|v| <$i>::from(v)).collect())
			}
		}
	)*}
}
