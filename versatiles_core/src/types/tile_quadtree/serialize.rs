//! Serialization and deserialization for [`TileQuadtree`].
//!
//! Format:
//! - Header: 1 byte zoom level
//! - Body: bitstream in DFS prefix order, 2 bits per node:
//!   - `00` = Empty
//!   - `01` = Full
//!   - `10` = Partial (followed by NW, NE, SW, SE children)
//!   - `11` = reserved
//! - Bits are packed MSB-first; the final byte is zero-padded.

use super::{Node, TileQuadtree};
use anyhow::{Result, bail, ensure};

impl TileQuadtree {
	/// Serialize this quadtree to bytes.
	///
	/// The first byte is the zoom level, followed by a packed 2-bit-per-node
	/// bitstream in DFS order.
	#[must_use]
	pub fn serialize(&self) -> Vec<u8> {
		let mut bits: Vec<u8> = Vec::new(); // each element is 0 or 1
		node_to_bits(&self.root, &mut bits);

		// Pack bits into bytes, MSB first
		let byte_count = bits.len().div_ceil(8);
		let mut bytes = Vec::with_capacity(1 + byte_count);
		bytes.push(self.zoom);
		let mut byte = 0u8;
		for (i, bit) in bits.iter().enumerate() {
			byte |= bit << (7 - (i % 8));
			if i % 8 == 7 {
				bytes.push(byte);
				byte = 0;
			}
		}
		if !bits.len().is_multiple_of(8) {
			bytes.push(byte);
		}
		bytes
	}

	/// Deserialize a quadtree from bytes produced by [`TileQuadtree::serialize`].
	///
	/// The `zoom` parameter must match the zoom level stored in the bytes.
	///
	/// # Errors
	/// Returns an error if the byte stream is malformed or zoom levels mismatch.
	pub fn deserialize(zoom: u8, bytes: &[u8]) -> Result<Self> {
		ensure!(!bytes.is_empty(), "empty byte slice");
		ensure!(
			bytes[0] == zoom,
			"zoom mismatch: header says {} but caller expects {}",
			bytes[0],
			zoom
		);

		// Unpack bits
		let bit_bytes = &bytes[1..];
		let bits: Vec<u8> = bit_bytes
			.iter()
			.flat_map(|b| (0..8u8).rev().map(move |i| (b >> i) & 1))
			.collect();

		let mut cursor = 0usize;
		let root = bits_to_node(&bits, &mut cursor)?;

		Ok(TileQuadtree { zoom, root })
	}
}

fn node_to_bits(node: &Node, bits: &mut Vec<u8>) {
	match node {
		Node::Empty => {
			bits.push(0);
			bits.push(0);
		}
		Node::Full => {
			bits.push(0);
			bits.push(1);
		}
		Node::Partial(children) => {
			bits.push(1);
			bits.push(0);
			for child in children.iter() {
				node_to_bits(child, bits);
			}
		}
	}
}

fn bits_to_node(bits: &[u8], cursor: &mut usize) -> Result<Node> {
	ensure!(*cursor + 2 <= bits.len(), "unexpected end of bitstream");
	let b0 = bits[*cursor];
	let b1 = bits[*cursor + 1];
	*cursor += 2;

	match (b0, b1) {
		(0, 0) => Ok(Node::Empty),
		(0, 1) => Ok(Node::Full),
		(1, 0) => {
			let nw = bits_to_node(bits, cursor)?;
			let ne = bits_to_node(bits, cursor)?;
			let sw = bits_to_node(bits, cursor)?;
			let se = bits_to_node(bits, cursor)?;
			Ok(Node::Partial(Box::new([nw, ne, sw, se])))
		}
		(1, 1) => bail!("reserved bit pattern 11 in bitstream"),
		_ => bail!("invalid bit values: {b0}, {b1}"),
	}
}
