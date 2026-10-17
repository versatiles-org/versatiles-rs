//! The two spellings of a source entry: a `[first, src]` pair, or a mapping.

use std::{fmt, marker::PhantomData};

use serde::{
	Deserialize, Deserializer,
	de::{self, IgnoredAny, MapAccess, SeqAccess, Visitor, value::MapAccessDeserializer},
};

/// Deserializes either a two-element sequence `[first, src]` or a mapping into
/// `T`.
///
/// The pair form is what the documentation shows for `tiles:` and `static:`,
/// and a derived `Deserialize` refused it: the mapping helpers deny unknown
/// fields and expect a map. A mapping is still handed to `T` itself, so its
/// error messages stay as they were.
pub(super) fn pair_or_mapping<'de, D, T>(
	deserializer: D,
	pair: fn(String, String) -> T,
	what: &'static str,
) -> Result<T, D::Error>
where
	D: Deserializer<'de>,
	T: Deserialize<'de>,
{
	struct PairOrMapping<T> {
		pair: fn(String, String) -> T,
		what: &'static str,
		marker: PhantomData<T>,
	}

	impl<'de, T: Deserialize<'de>> Visitor<'de> for PairOrMapping<T> {
		type Value = T;

		fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
			f.write_str(self.what)
		}

		fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<T, A::Error> {
			let first: String = seq.next_element()?.ok_or_else(|| de::Error::invalid_length(0, &self))?;
			let src: String = seq.next_element()?.ok_or_else(|| de::Error::invalid_length(1, &self))?;
			if seq.next_element::<IgnoredAny>()?.is_some() {
				return Err(de::Error::invalid_length(3, &self));
			}
			Ok((self.pair)(first, src))
		}

		fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<T, A::Error> {
			T::deserialize(MapAccessDeserializer::new(map))
		}
	}

	deserializer.deserialize_any(PairOrMapping {
		pair,
		what,
		marker: PhantomData,
	})
}
