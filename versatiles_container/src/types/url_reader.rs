//! Opening a URL as a [`DataReader`].
//!
//! The one place that knows which URL schemes can be read and how: the
//! container registry opens remote containers through it, and the pipeline
//! reads remote files through it. Whether `sftp` works is decided here, by this
//! crate's `sftp` feature, so a caller in another crate gets SFTP exactly when
//! the binary enables it.

use std::path::Path;

use anyhow::{Context, Result, bail};
use reqwest::Url;
#[cfg(feature = "sftp")]
use versatiles_core::io::DataReaderSftp;
use versatiles_core::io::{DataReader, DataReaderHttp, url_for_display};

use crate::TilesRuntime;

/// Opens `url` for reading: `http` and `https` always, `sftp` when compiled in.
///
/// For `sftp`, `ssh_identity` is the key for this URL alone; without one, the
/// runtime's process-wide key is used. The rest of the authentication order
/// (password in the URL, agent, `~/.ssh/config`, default keys) is
/// `sftp_utils::open_session`'s.
///
/// # Errors
///
/// Errors for any other scheme (naming it), and when the reader cannot be
/// created — for SFTP that includes the SSH handshake.
#[cfg_attr(
	not(feature = "sftp"),
	allow(
		unused_variables,
		reason = "`ssh_identity` and `runtime` are read only by the SFTP branch, compiled out without `sftp`"
	)
)]
pub async fn open_url_reader(url: &Url, ssh_identity: Option<&Path>, runtime: &TilesRuntime) -> Result<DataReader> {
	Ok(match url.scheme() {
		#[cfg(feature = "sftp")]
		"sftp" => {
			// The SSH handshake is async now, so concurrent opens (e.g.
			// from `from_stacked_raster` building several sub-pipelines
			// at once) already interleave on one worker — no
			// `spawn_blocking` needed to keep them from serializing.
			// A key named on the source itself wins over the process-wide one.
			let identity = ssh_identity.or_else(|| runtime.ssh_identity());
			let reader = DataReaderSftp::open(url, identity)
				.await
				.with_context(|| format!("Failed to create SFTP data reader for URL '{}'", url_for_display(url)))?;
			Box::new(reader)
		}
		"http" | "https" => Box::new(
			DataReaderHttp::try_from(url)
				.with_context(|| format!("Failed to create HTTP data reader for URL '{}'", url_for_display(url)))?,
		),
		scheme => bail!("unsupported URL scheme '{scheme}' in '{}'", url_for_display(url)),
	})
}

#[cfg(test)]
mod tests {
	use super::*;

	#[tokio::test]
	async fn an_unknown_scheme_is_named() {
		let url = Url::parse("ftp://user:secret@example.org/tiles.versatiles").unwrap();
		let error = open_url_reader(&url, None, &TilesRuntime::new_silent())
			.await
			.expect_err("ftp is not readable");
		let message = format!("{error:#}");
		assert!(message.contains("unsupported URL scheme 'ftp'"), "got: {message}");
		assert!(!message.contains("secret"), "the password leaked: {message}");
	}

	#[tokio::test]
	async fn http_opens_without_touching_the_network() {
		let url = Url::parse("https://example.org/tiles.versatiles").unwrap();
		let reader = open_url_reader(&url, None, &TilesRuntime::new_silent()).await.unwrap();
		assert!(reader.name().contains("example.org"), "got: {}", reader.name());
	}
}
