//! AVIF decoding, written entirely in Rust and compiled into the binary.
//!
//! Three crates share the work. `avif-parse` reads the AVIF container and hands out the AV1
//! payload of the colour image and, if there is one, of its alpha image. [`rav1d`], a Rust port
//! of the dav1d decoder, turns each payload into YUV planes. `yuv` converts those planes to RGB.
//! rav1d is built without its assembly, so building needs no assembler.
//!
//! rav1d only exposes dav1d's C API. [`Decoder`] and [`Picture`] wrap the few calls needed here
//! in types that release what they hold when dropped.
//!
//! Only 8-bit images are decoded: rav1d is built with `bitdepth_8` alone, and the rest of this
//! crate is 8-bit anyway.

use std::{mem::MaybeUninit, ptr::NonNull};

use anyhow::{Context, Result, anyhow, bail, ensure};
use image::{DynamicImage, ImageBuffer};
use rav1d::{
	Dav1dResult,
	include::dav1d::{
		data::Dav1dData,
		dav1d::{Dav1dContext, Dav1dSettings},
		headers::{
			DAV1D_MC_BT470BG, DAV1D_MC_BT601, DAV1D_MC_BT709, DAV1D_MC_BT2020_CL, DAV1D_MC_BT2020_NCL, DAV1D_MC_FCC,
			DAV1D_MC_IDENTITY, DAV1D_MC_SMPTE240, DAV1D_MC_UNKNOWN, DAV1D_PIXEL_LAYOUT_I400, DAV1D_PIXEL_LAYOUT_I420,
			DAV1D_PIXEL_LAYOUT_I422, DAV1D_PIXEL_LAYOUT_I444, Dav1dPixelLayout,
		},
		picture::Dav1dPicture,
	},
	src::lib::{
		dav1d_close, dav1d_data_create, dav1d_data_unref, dav1d_default_settings, dav1d_get_picture, dav1d_open,
		dav1d_picture_unref, dav1d_send_data,
	},
};
use yuv::{YuvPlanarImage, YuvRange, YuvStandardMatrix};

use crate::format::all::{MAX_BYTES_PER_PIXEL, MAX_DECODED_BYTES, ensure_decodable};

/// Most pixels rav1d may allocate for one frame: the crate's byte ceiling at four bytes a pixel.
///
/// The header check in [`decode`] already refuses larger images. This repeats the bound inside
/// the decoder, so a payload whose frame header disagrees with its sequence header is caught too.
#[expect(clippy::cast_possible_truncation, reason = "512 MiB / 4 is 2^27, which fits in u32")]
const FRAME_SIZE_LIMIT: u32 = (MAX_DECODED_BYTES / MAX_BYTES_PER_PIXEL) as u32;

/// Decode an AVIF file into an 8-bit [`DynamicImage`].
///
/// A colour image becomes `Rgb8`, or `Rgba8` with an alpha image. A monochrome image becomes
/// `Luma8`, or `LumaA8` with an alpha image.
pub(super) fn decode(data: &[u8]) -> Result<DynamicImage> {
	let mut reader = data;
	let avif = avif_parse::read_avif(&mut reader).map_err(|e| anyhow!("invalid AVIF file: {e}"))?;

	let meta = avif
		.primary_item_metadata()
		.map_err(|e| anyhow!("invalid AV1 sequence header: {e}"))?;
	ensure!(
		meta.bit_depth == 8,
		"only 8-bit AVIF images can be decoded, this one has {} bits per channel",
		meta.bit_depth
	);
	ensure_decodable(
		meta.max_frame_width.get(),
		meta.max_frame_height.get(),
		MAX_BYTES_PER_PIXEL,
	)?;

	let decoder = Decoder::new()?;
	let has_alpha = avif.alpha_item.is_some();

	let color = decoder.decode(&avif.primary_item)?;
	let (width, height) = color.size()?;
	let monochrome = color.layout() == DAV1D_PIXEL_LAYOUT_I400;
	let channels: usize = match (monochrome, has_alpha) {
		(true, false) => 1,
		(true, true) => 2,
		(false, false) => 3,
		(false, true) => 4,
	};
	let mut pixels = vec![0u8; width as usize * height as usize * channels];
	if monochrome {
		let luma = color.plane(0)?;
		let full_range = color.full_range();
		for (y, row) in pixels.chunks_exact_mut(width as usize * channels).enumerate() {
			for (px, &v) in row.chunks_exact_mut(channels).zip(luma.row(y)) {
				px[0] = expand_range(v, full_range);
			}
		}
	} else {
		color.to_rgb(&mut pixels, has_alpha)?;
	}
	drop(color);

	if let Some(alpha_item) = &avif.alpha_item {
		let alpha = decoder.decode(alpha_item)?;
		ensure!(
			alpha.size()? == (width, height),
			"AVIF alpha image is {:?}, but the colour image is {width}x{height}",
			alpha.size()?
		);
		let plane = alpha.plane(0)?;
		let full_range = alpha.full_range();
		for (y, row) in pixels.chunks_exact_mut(width as usize * channels).enumerate() {
			for (px, &v) in row.chunks_exact_mut(channels).zip(plane.row(y)) {
				px[channels - 1] = expand_range(v, full_range);
			}
		}
		if avif.premultiplied_alpha {
			unpremultiply(&mut pixels, channels);
		}
	}

	Ok(match channels {
		1 => DynamicImage::ImageLuma8(ImageBuffer::from_raw(width, height, pixels).context("AVIF buffer")?),
		2 => DynamicImage::ImageLumaA8(ImageBuffer::from_raw(width, height, pixels).context("AVIF buffer")?),
		3 => DynamicImage::ImageRgb8(ImageBuffer::from_raw(width, height, pixels).context("AVIF buffer")?),
		_ => DynamicImage::ImageRgba8(ImageBuffer::from_raw(width, height, pixels).context("AVIF buffer")?),
	})
}

/// Map an 8-bit sample to the full 0..=255 range. Limited range puts black at 16 and white at 235.
#[expect(
	clippy::cast_possible_truncation,
	reason = "the result is at most 255 by construction"
)]
fn expand_range(v: u8, full_range: bool) -> u8 {
	if full_range {
		v
	} else {
		let v = u32::from(v).saturating_sub(16).min(219);
		((v * 255 + 109) / 219) as u8
	}
}

/// Undo premultiplied alpha in place. The last channel of each pixel is alpha.
fn unpremultiply(pixels: &mut [u8], channels: usize) {
	for px in pixels.chunks_exact_mut(channels) {
		let (color, alpha) = px.split_at_mut(channels - 1);
		let a = u16::from(alpha[0]);
		if a != 0 && a != 255 {
			for c in color {
				*c = (u16::from(*c) * 255 / a).min(255) as u8;
			}
		}
	}
}

/// Turn a rav1d status into a `Result`. rav1d reports success as 0 and errors as a negated `errno`.
fn check(res: Dav1dResult, what: &str) -> Result<()> {
	if res.0 == 0 { Ok(()) } else { Err(failure(res, what)) }
}

/// The error for a failed rav1d call.
fn failure(res: Dav1dResult, what: &str) -> anyhow::Error {
	anyhow!("AV1 decoder failed while {what} (error {})", -res.0)
}

/// A rav1d decoder context, closed when dropped.
struct Decoder {
	ctx: Option<Dav1dContext>,
}

impl Decoder {
	/// Open a single-threaded decoder. Tiles are decoded in parallel one level up, so threads
	/// inside one decode would only compete with those.
	#[expect(
		unsafe_code,
		reason = "rav1d's C API. `dav1d_default_settings` fills every field of the settings, and `dav1d_open` gets pointers to two locals that outlive the call"
	)]
	fn new() -> Result<Self> {
		let mut settings = MaybeUninit::<Dav1dSettings>::uninit();
		// SAFETY: `dav1d_default_settings` writes a complete `Dav1dSettings`.
		let mut settings = unsafe {
			dav1d_default_settings(NonNull::from(&mut settings).cast());
			settings.assume_init()
		};
		settings.n_threads = 1;
		// Still images have one frame, so there is nothing to pipeline.
		settings.max_frame_delay = 1;
		// Film grain is part of the image as the encoder meant it.
		settings.apply_grain = 1;
		settings.frame_size_limit = FRAME_SIZE_LIMIT;

		let mut ctx = None;
		// SAFETY: both pointers are to live locals; `dav1d_open` writes the context into `ctx`.
		let res = unsafe { dav1d_open(Some(NonNull::from(&mut ctx)), Some(NonNull::from(&mut settings))) };
		check(res, "opening")?;
		Ok(Self { ctx })
	}

	/// Decode one AVIF item, which is one AV1 temporal unit, and return its picture.
	///
	/// A layered item decodes to one picture per layer. Each layer refines the one before, so
	/// the last picture is the complete image and is the one returned.
	#[expect(
		unsafe_code,
		reason = "rav1d's C API. The data buffer is allocated by rav1d with the length copied into it and is unref'd on every path; pictures are owned by `Picture`, which unrefs them"
	)]
	fn decode(&self, av1: &[u8]) -> Result<Picture> {
		let ctx = self.ctx.context("AV1 decoder is closed")?;

		let mut data = Dav1dData::default();
		// SAFETY: `data` is a live local; rav1d allocates `av1.len()` bytes and points `data` at them.
		let buf = unsafe { dav1d_data_create(Some(NonNull::from(&mut data)), av1.len()) };
		ensure!(!buf.is_null(), "AV1 decoder could not allocate {} bytes", av1.len());
		// SAFETY: `buf` was just allocated with room for exactly `av1.len()` bytes.
		unsafe { std::ptr::copy_nonoverlapping(av1.as_ptr(), buf, av1.len()) };

		let mut last: Option<Picture> = None;
		let sent = loop {
			// SAFETY: `ctx` is open and `data` holds the buffer created above.
			let res = unsafe { dav1d_send_data(Some(ctx), Some(NonNull::from(&mut data))) };
			if res.0 == 0 {
				break Ok(());
			}
			// The decoder may refuse new data while it still holds a finished picture.
			// Take that picture out and try again; if there is none, the data is bad.
			match Self::get_picture(ctx) {
				Ok(pic) => last = Some(pic),
				Err(_) => break check(res, "reading the AV1 data"),
			}
		};
		// SAFETY: `data` is either consumed (then this is a no-op) or still ours to release.
		unsafe { dav1d_data_unref(Some(NonNull::from(&mut data))) };
		sent?;

		loop {
			match Self::get_picture(ctx) {
				Ok(pic) => last = Some(pic),
				Err(e) => return last.ok_or(e),
			}
		}
	}

	/// Take the next finished picture out of the decoder.
	#[expect(
		unsafe_code,
		reason = "rav1d's C API. The picture is a live local; a picture rav1d did not fill is unref'd, which is a no-op on a default picture"
	)]
	fn get_picture(ctx: Dav1dContext) -> Result<Picture> {
		let mut pic = Dav1dPicture::default();
		// SAFETY: `ctx` is open and `pic` is a live local.
		let res = unsafe { dav1d_get_picture(Some(ctx), Some(NonNull::from(&mut pic))) };
		if res.0 == 0 {
			Ok(Picture(pic))
		} else {
			// SAFETY: unref on a picture that holds no buffers does nothing.
			unsafe { dav1d_picture_unref(Some(NonNull::from(&mut pic))) };
			Err(failure(res, "decoding the AV1 frame"))
		}
	}
}

impl Drop for Decoder {
	#[expect(
		unsafe_code,
		reason = "closes the context `dav1d_open` created; it is not used afterwards"
	)]
	fn drop(&mut self) {
		// SAFETY: `ctx` came from `dav1d_open`, and `dav1d_close` sets it to `None`.
		unsafe { dav1d_close(Some(NonNull::from(&mut self.ctx))) };
	}
}

/// A decoded picture, released when dropped.
struct Picture(Dav1dPicture);

impl Drop for Picture {
	#[expect(
		unsafe_code,
		reason = "releases a picture rav1d handed out; it is not used afterwards"
	)]
	fn drop(&mut self) {
		// SAFETY: the picture came from `dav1d_get_picture` and is released exactly once.
		unsafe { dav1d_picture_unref(Some(NonNull::from(&mut self.0))) };
	}
}

/// One plane of a [`Picture`], borrowed from the decoder's buffer.
struct Plane<'a> {
	buf: &'a [u8],
	stride: usize,
	width: usize,
	height: usize,
}

impl Plane<'_> {
	fn row(&self, y: usize) -> &[u8] {
		&self.buf[y * self.stride..y * self.stride + self.width]
	}
}

impl Picture {
	fn size(&self) -> Result<(u32, u32)> {
		let w = u32::try_from(self.0.p.w).context("AV1 picture has a negative width")?;
		let h = u32::try_from(self.0.p.h).context("AV1 picture has a negative height")?;
		ensure!(w > 0 && h > 0, "AV1 picture is empty ({w}x{h})");
		Ok((w, h))
	}

	fn layout(&self) -> Dav1dPixelLayout {
		self.0.p.layout
	}

	/// Whether samples use the full 0..=255 range rather than the limited 16..=235.
	#[expect(
		unsafe_code,
		reason = "reads the sequence header rav1d attached to the picture, which lives as long as the picture"
	)]
	fn full_range(&self) -> bool {
		// SAFETY: `seq_hdr` points into the picture's reference-counted header.
		self.0.seq_hdr.is_some_and(|h| unsafe { h.as_ref() }.color_range != 0)
	}

	#[expect(
		unsafe_code,
		reason = "reads the sequence header rav1d attached to the picture, which lives as long as the picture"
	)]
	fn matrix(&self) -> u32 {
		// SAFETY: as in `full_range`.
		self.0.seq_hdr.map_or(DAV1D_MC_UNKNOWN, |h| unsafe { h.as_ref() }.mtrx)
	}

	/// Borrow plane 0 (Y), 1 (U) or 2 (V).
	#[expect(
		unsafe_code,
		reason = "builds a slice over a plane rav1d allocated; see the SAFETY note for the length"
	)]
	fn plane(&self, index: usize) -> Result<Plane<'_>> {
		ensure!(
			self.0.p.bpc == 8,
			"AV1 picture has {} bits per channel, not 8",
			self.0.p.bpc
		);
		let (w, h) = self.size()?;
		let (ss_x, ss_y) = match (index, self.layout()) {
			(0, _) | (_, DAV1D_PIXEL_LAYOUT_I444) => (0, 0),
			(_, DAV1D_PIXEL_LAYOUT_I422) => (1, 0),
			(_, DAV1D_PIXEL_LAYOUT_I420) => (1, 1),
			(_, layout) => bail!("AV1 picture with pixel layout {layout} has no plane {index}"),
		};
		let width = (w as usize + ss_x) >> ss_x;
		let height = (h as usize + ss_y) >> ss_y;
		// rav1d keeps one stride for luma and one for both chroma planes.
		let stride =
			usize::try_from(self.0.stride[usize::from(index != 0)]).context("AV1 plane has a negative stride")?;
		ensure!(
			stride >= width,
			"AV1 plane stride {stride} is shorter than its width {width}"
		);
		let ptr = self.0.data[index].context("AV1 picture lacks a plane")?;
		// SAFETY: rav1d's allocator gives each plane `stride` bytes for each of at least the
		// picture's height rounded up to 128 rows (`src/picture.rs`), so `stride * height` bytes
		// are inside the buffer, which lives as long as the picture this borrows from.
		let buf = unsafe { std::slice::from_raw_parts(ptr.as_ptr().cast::<u8>(), stride * height) };
		Ok(Plane {
			buf,
			stride,
			width,
			height,
		})
	}

	/// Convert the picture's YUV planes into `out`, three bytes a pixel, or four with `has_alpha`.
	/// The alpha bytes are left for the caller to fill.
	#[expect(
		clippy::cast_possible_truncation,
		reason = "plane sizes and strides are bounded by FRAME_SIZE_LIMIT, which fits in u32"
	)]
	fn to_rgb(&self, out: &mut [u8], has_alpha: bool) -> Result<()> {
		let (y, u, v) = (self.plane(0)?, self.plane(1)?, self.plane(2)?);
		let planar = YuvPlanarImage {
			y_plane: y.buf,
			y_stride: y.stride as u32,
			u_plane: u.buf,
			u_stride: u.stride as u32,
			v_plane: v.buf,
			v_stride: v.stride as u32,
			width: y.width as u32,
			height: y.height as u32,
		};
		let range = if self.full_range() {
			YuvRange::Full
		} else {
			YuvRange::Limited
		};
		let channels: u32 = if has_alpha { 4 } else { 3 };
		let stride = planar.width * channels;

		let matrix = match self.matrix() {
			DAV1D_MC_IDENTITY => {
				ensure!(
					self.layout() == DAV1D_PIXEL_LAYOUT_I444,
					"AVIF uses the identity matrix with chroma subsampling"
				);
				let res = if has_alpha {
					yuv::gbr_to_rgba(&planar, out, stride, range)
				} else {
					yuv::gbr_to_rgb(&planar, out, stride, range)
				};
				return res.map_err(|e| anyhow!("converting AVIF colours: {e}"));
			}
			DAV1D_MC_BT709 => YuvStandardMatrix::Bt709,
			// BT.470 B/G uses the BT.601 matrix. Unspecified is BT.601 in AVIF practice.
			DAV1D_MC_BT601 | DAV1D_MC_BT470BG | DAV1D_MC_UNKNOWN => YuvStandardMatrix::Bt601,
			DAV1D_MC_SMPTE240 => YuvStandardMatrix::Smpte240,
			DAV1D_MC_FCC => YuvStandardMatrix::Fcc,
			DAV1D_MC_BT2020_NCL | DAV1D_MC_BT2020_CL => YuvStandardMatrix::Bt2020,
			other => bail!("AVIF matrix coefficients {other} are not supported"),
		};

		// Subsampled chroma is upsampled nearest-neighbour. On a 4:2:0 satellite tile this
		// scores 37.2 dB PSNR against the source, libavif's default bilinear 37.7 dB, and the
		// `yuv` crate's `_bilinear` functions only 35.9 dB, so those are not used.
		let res = match (self.layout(), has_alpha) {
			(DAV1D_PIXEL_LAYOUT_I444, false) => yuv::yuv444_to_rgb(&planar, out, stride, range, matrix),
			(DAV1D_PIXEL_LAYOUT_I444, true) => yuv::yuv444_to_rgba(&planar, out, stride, range, matrix),
			(DAV1D_PIXEL_LAYOUT_I422, false) => yuv::yuv422_to_rgb(&planar, out, stride, range, matrix),
			(DAV1D_PIXEL_LAYOUT_I422, true) => yuv::yuv422_to_rgba(&planar, out, stride, range, matrix),
			(DAV1D_PIXEL_LAYOUT_I420, false) => yuv::yuv420_to_rgb(&planar, out, stride, range, matrix),
			(DAV1D_PIXEL_LAYOUT_I420, true) => yuv::yuv420_to_rgba(&planar, out, stride, range, matrix),
			(layout, _) => bail!("AVIF pixel layout {layout} is not supported"),
		};
		res.map_err(|e| anyhow!("converting AVIF colours: {e}"))
	}
}

#[cfg(test)]
mod tests {
	//! Files written by libavif's `avifenc`, checked against `avifdec -d 8 -u nearest`.
	//!
	//! They cover what this crate's own encoder never writes: chroma subsampling, limited range,
	//! other matrices, monochrome, premultiplied alpha and the identity matrix. The colour
	//! conversion has SIMD paths per CPU, so this is the test that catches one rounding
	//! differently.

	use rstest::rstest;
	use versatiles_core::{Blob, TileFormat};

	use super::*;

	fn fixture(name: &str) -> Vec<u8> {
		let path = format!("{}/../testdata/avif/{name}", env!("CARGO_MANIFEST_DIR"));
		std::fs::read(&path).unwrap_or_else(|e| panic!("reading {path}: {e}"))
	}

	/// `max_diff` is the largest channel difference to the reference on Apple Silicon.
	/// The limit allows one more, for SIMD paths on other CPUs that round differently.
	#[rstest]
	#[case::yuv420_limited("yuv420_limited", image::ColorType::Rgb8, 1)]
	#[case::yuv422_full("yuv422_full", image::ColorType::Rgb8, 1)]
	#[case::yuv444_bt709("yuv444_bt709", image::ColorType::Rgb8, 1)]
	#[case::mono("mono", image::ColorType::L8, 0)]
	#[case::lossless("lossless", image::ColorType::Rgb8, 0)]
	#[case::alpha_yuv420("alpha_yuv420", image::ColorType::Rgba8, 1)]
	#[case::alpha_premultiplied("alpha_premultiplied", image::ColorType::Rgba8, 2)]
	fn matches_libavif(#[case] name: &str, #[case] color: image::ColorType, #[case] max_diff: u8) -> Result<()> {
		let decoded = decode(&fixture(&format!("{name}.avif")))?;
		let reference = crate::format::decode(&Blob::from(fixture(&format!("{name}.png"))), TileFormat::PNG)?;

		assert_eq!(decoded.color(), color);
		assert_eq!(decoded.color(), reference.color());
		assert_eq!((decoded.width(), decoded.height()), (64, 64));

		let diff = decoded
			.as_bytes()
			.iter()
			.zip(reference.as_bytes())
			.map(|(a, b)| a.abs_diff(*b))
			.max()
			.unwrap();
		assert!(
			diff <= max_diff + 1,
			"{name}: max channel difference {diff}, expected at most {max_diff} + 1"
		);
		Ok(())
	}

	#[test]
	fn refuses_10_bit() {
		let msg = format!("{:#}", decode(&fixture("depth10.avif")).unwrap_err());
		assert!(msg.contains("only 8-bit AVIF images"), "got: {msg}");
	}
}
