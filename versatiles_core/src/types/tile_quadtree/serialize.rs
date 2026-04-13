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
		let mut out = vec![self.level]; // header
		let mut byte: u8 = 0;
		let mut bit_pos: u8 = 0;
		write_node(&self.root, &mut out, &mut byte, &mut bit_pos);
		if bit_pos > 0 {
			out.push(byte); // flush remaining bits
		}
		out
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

		let mut reader = BitReader {
			data: &bytes[1..],
			byte_idx: 0,
			bit_idx: 0,
		};
		let root = read_node(&mut reader)?;

		Ok(TileQuadtree { level: zoom, root })
	}
}

/// Write a node's 2-bit tag (and recurse for Partial) directly into the output buffer.
fn write_node(node: &Node, out: &mut Vec<u8>, byte: &mut u8, bit_pos: &mut u8) {
	let bits: u8 = match node {
		Node::Empty => 0b00,
		Node::Full => 0b01,
		Node::Partial(_) => 0b10,
	};
	// Write 2 bits MSB-first
	for shift in [1u8, 0u8] {
		let b = (bits >> shift) & 1;
		*byte |= b << (7 - *bit_pos);
		*bit_pos += 1;
		if *bit_pos == 8 {
			out.push(*byte);
			*byte = 0;
			*bit_pos = 0;
		}
	}
	if let Node::Partial(children) = node {
		for child in children.iter() {
			write_node(child, out, byte, bit_pos);
		}
	}
}

/// Cursor-based bit reader: reads bits MSB-first from a byte slice.
struct BitReader<'a> {
	data: &'a [u8],
	byte_idx: usize,
	bit_idx: u8, // 0..8, MSB first
}

impl BitReader<'_> {
	fn read_bit(&mut self) -> Option<u8> {
		let byte = *self.data.get(self.byte_idx)?;
		let bit = (byte >> (7 - self.bit_idx)) & 1;
		self.bit_idx += 1;
		if self.bit_idx == 8 {
			self.bit_idx = 0;
			self.byte_idx += 1;
		}
		Some(bit)
	}

	fn read_2bits(&mut self) -> Option<u8> {
		let b0 = self.read_bit()?;
		let b1 = self.read_bit()?;
		Some((b0 << 1) | b1)
	}
}

fn read_node(reader: &mut BitReader<'_>) -> Result<Node> {
	let tag = reader
		.read_2bits()
		.ok_or_else(|| anyhow::anyhow!("unexpected end of bitstream"))?;
	match tag {
		0b00 => Ok(Node::Empty),
		0b01 => Ok(Node::Full),
		0b10 => {
			let nw = read_node(reader)?;
			let ne = read_node(reader)?;
			let sw = read_node(reader)?;
			let se = read_node(reader)?;
			Ok(Node::Partial(Box::new([nw, ne, sw, se])))
		}
		0b11 => bail!("reserved bit pattern 11 in bitstream"),
		_ => bail!("invalid 2-bit value: {tag}"),
	}
}
