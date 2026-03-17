use super::DataReaderTrait;
use crate::{Blob, ByteRange};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use ssh2::Session;
use std::{
	io::{Read, Seek, SeekFrom},
	net::TcpStream,
	sync::Mutex,
};

/// A struct that provides reading capabilities from a remote file via SFTP.
pub struct DataReaderSftp {
	file: Mutex<ssh2::File>,
	size: u64,
	name: String,
	// Keep session alive for the lifetime of the reader
	_session: Session,
}

impl std::fmt::Debug for DataReaderSftp {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("DataReaderSftp").field("name", &self.name).finish()
	}
}

impl DataReaderSftp {
	/// Opens a remote file for reading via SFTP.
	///
	/// # Arguments
	/// * `url` - An SFTP URL of the form `sftp://[user[:pass]@]host[:port]/path`
	///
	/// # Authentication priority
	/// 1. Credentials in URL (password auth)
	/// 2. SSH agent
	/// 3. Default key files (~/.ssh/id_ed25519, id_rsa, id_ecdsa)
	pub fn from_url(url: &str) -> Result<Box<Self>> {
		// Reuse the shared URL parser and auth helpers from the writer module
		let parsed = super::data_writer_sftp::parse_sftp_url(url)?;
		let username = parsed.user.as_deref().unwrap_or("root");

		// Connect TCP
		let tcp = TcpStream::connect((&*parsed.host, parsed.port))
			.with_context(|| format!("failed to connect to {}:{}", parsed.host, parsed.port))?;

		// SSH handshake
		let mut session = Session::new()?;
		session.set_tcp_stream(tcp);
		session.handshake()?;

		// Authenticate
		let auth_result = if let Some(ref password) = parsed.password {
			session.userauth_password(username, password)
		} else {
			Err(ssh2::Error::from_errno(ssh2::ErrorCode::Session(-1)))
		};

		if auth_result.is_err() {
			let agent_result = super::data_writer_sftp::try_agent_auth(&session, username);
			if agent_result.is_err() {
				super::data_writer_sftp::try_key_auth(&session, username).with_context(|| {
					format!(
						"all authentication methods failed for {username}@{}:{}",
						parsed.host, parsed.port
					)
				})?;
			}
		}

		if !session.authenticated() {
			bail!(
				"SSH authentication failed for {username}@{}:{}",
				parsed.host,
				parsed.port
			);
		}

		// Open SFTP channel and open file for reading
		let sftp = session.sftp()?;
		let stat = sftp
			.stat(&parsed.path)
			.with_context(|| format!("failed to stat remote file {:?}", parsed.path))?;
		let size = stat.size.unwrap_or(0);

		let file = sftp
			.open(&parsed.path)
			.with_context(|| format!("failed to open remote file {:?}", parsed.path))?;

		// Build a sanitized name (without credentials)
		let name = format!("sftp://{}:{}{}", parsed.host, parsed.port, parsed.path.display());

		Ok(Box::new(DataReaderSftp {
			file: Mutex::new(file),
			size,
			name,
			_session: session,
		}))
	}

	/// Returns the remote path extracted from an SFTP URL (for extension detection).
	#[must_use]
	pub fn path_from_url(url: &str) -> Option<std::path::PathBuf> {
		super::data_writer_sftp::parse_sftp_url(url).ok().map(|u| u.path)
	}
}

#[async_trait]
impl DataReaderTrait for DataReaderSftp {
	async fn read_range(&self, range: &ByteRange) -> Result<Blob> {
		let mut file = self.file.lock().map_err(|e| anyhow::anyhow!("lock poisoned: {e}"))?;
		file.seek(SeekFrom::Start(range.offset))?;
		let mut buffer = vec![0u8; range.length as usize];
		file.read_exact(&mut buffer).with_context(|| {
			format!(
				"failed to read {} bytes at offset {} from '{}'",
				range.length, range.offset, self.name
			)
		})?;
		Ok(Blob::from(buffer))
	}

	async fn read_all(&self) -> Result<Blob> {
		self.read_range(&ByteRange::new(0, self.size)).await
	}

	fn get_name(&self) -> &str {
		&self.name
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_path_from_url() {
		assert_eq!(
			DataReaderSftp::path_from_url("sftp://host/data/tiles.versatiles"),
			Some(std::path::PathBuf::from("/data/tiles.versatiles"))
		);
	}

	#[test]
	fn test_path_from_url_invalid() {
		assert_eq!(DataReaderSftp::path_from_url("not-sftp"), None);
	}
}
