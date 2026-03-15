use super::DataWriterTrait;
use crate::{Blob, ByteRange};
use anyhow::{Context, Result, bail};
use ssh2::Session;
use std::{
	io::{Seek, SeekFrom, Write},
	net::TcpStream,
	path::PathBuf,
};

/// A struct that provides writing capabilities to a remote file via SFTP.
pub struct DataWriterSftp {
	file: ssh2::File,
	position: u64,
	// Keep session alive for the lifetime of the writer
	_session: Session,
}

/// Parsed components of an SFTP URL.
struct SftpUrl {
	user: Option<String>,
	password: Option<String>,
	host: String,
	port: u16,
	path: PathBuf,
}

fn parse_sftp_url(url: &str) -> Result<SftpUrl> {
	let rest = url.strip_prefix("sftp://").context("URL must start with sftp://")?;

	// Split into authority and path at the first '/'
	let (authority, path) = rest
		.split_once('/')
		.context("SFTP URL must contain a path after host (sftp://host/path)")?;

	let path = PathBuf::from(format!("/{path}"));

	// Parse authority: [user[:pass]@]host[:port]
	let (userinfo, hostport) = if let Some(idx) = authority.rfind('@') {
		(Some(&authority[..idx]), &authority[idx + 1..])
	} else {
		(None, authority)
	};

	let (user, password) = if let Some(ui) = userinfo {
		if let Some((u, p)) = ui.split_once(':') {
			(Some(u.to_string()), Some(p.to_string()))
		} else {
			(Some(ui.to_string()), None)
		}
	} else {
		(None, None)
	};

	let (host, port) = if let Some((h, p)) = hostport.rsplit_once(':') {
		let port: u16 = p.parse().context("invalid port number in SFTP URL")?;
		(h.to_string(), port)
	} else {
		(hostport.to_string(), 22)
	};

	if host.is_empty() {
		bail!("SFTP URL has empty host");
	}

	Ok(SftpUrl {
		user,
		password,
		host,
		port,
		path,
	})
}

/// Try authenticating with the SSH agent.
fn try_agent_auth(session: &Session, username: &str) -> Result<()> {
	let mut agent = session.agent()?;
	agent.connect()?;
	agent.list_identities()?;
	for identity in agent.identities()? {
		if agent.userauth(username, &identity).is_ok() {
			return Ok(());
		}
	}
	bail!("SSH agent has no suitable identities for user '{username}'")
}

/// Try authenticating with default key files.
fn try_key_auth(session: &Session, username: &str) -> Result<()> {
	let home = dirs_home()?;
	let key_files = [
		home.join(".ssh/id_ed25519"),
		home.join(".ssh/id_rsa"),
		home.join(".ssh/id_ecdsa"),
	];

	for key_path in &key_files {
		if key_path.exists() && session.userauth_pubkey_file(username, None, key_path, None).is_ok() {
			return Ok(());
		}
	}
	bail!("no suitable SSH key found in ~/.ssh/")
}

fn dirs_home() -> Result<PathBuf> {
	home_dir().context("could not determine home directory")
}

/// Cross-platform home directory lookup.
fn home_dir() -> Option<PathBuf> {
	#[cfg(unix)]
	{
		std::env::var_os("HOME").map(PathBuf::from)
	}
	#[cfg(not(unix))]
	{
		std::env::var_os("USERPROFILE").map(PathBuf::from)
	}
}

impl DataWriterSftp {
	/// Opens a remote file for writing via SFTP.
	///
	/// # Arguments
	/// * `url` - An SFTP URL of the form `sftp://[user[:pass]@]host[:port]/path`
	///
	/// # Authentication priority
	/// 1. Credentials in URL (password auth)
	/// 2. SSH agent
	/// 3. Default key files (~/.ssh/id_ed25519, id_rsa, id_ecdsa)
	pub fn from_url(url: &str) -> Result<Self> {
		let parsed = parse_sftp_url(url)?;
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
			let agent_result = try_agent_auth(&session, username);
			if agent_result.is_err() {
				try_key_auth(&session, username).with_context(|| {
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

		// Open SFTP channel and create file
		let sftp = session.sftp()?;
		let file = sftp
			.create(&parsed.path)
			.with_context(|| format!("failed to create remote file {:?}", parsed.path))?;

		Ok(DataWriterSftp {
			file,
			position: 0,
			_session: session,
		})
	}

	/// Returns the remote path extracted from an SFTP URL (for extension detection).
	#[must_use]
	pub fn path_from_url(url: &str) -> Option<PathBuf> {
		parse_sftp_url(url).ok().map(|u| u.path)
	}
}

impl DataWriterTrait for DataWriterSftp {
	fn append(&mut self, blob: &Blob) -> Result<ByteRange> {
		let pos = self.position;
		self.file.write_all(blob.as_slice())?;
		let len = blob.len();
		self.position += len;
		Ok(ByteRange::new(pos, len))
	}

	fn write_start(&mut self, blob: &Blob) -> Result<()> {
		let saved = self.position;
		self.file.seek(SeekFrom::Start(0))?;
		self.file.write_all(blob.as_slice())?;
		self.file.seek(SeekFrom::Start(saved))?;
		Ok(())
	}

	fn get_position(&mut self) -> Result<u64> {
		Ok(self.position)
	}

	fn set_position(&mut self, position: u64) -> Result<()> {
		self.file.seek(SeekFrom::Start(position))?;
		self.position = position;
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::path::Path;

	#[test]
	fn test_parse_sftp_url_full() {
		let parsed = parse_sftp_url("sftp://user:pass@host.example.com:2222/data/output.versatiles").unwrap();
		assert_eq!(parsed.user.as_deref(), Some("user"));
		assert_eq!(parsed.password.as_deref(), Some("pass"));
		assert_eq!(parsed.host, "host.example.com");
		assert_eq!(parsed.port, 2222);
		assert_eq!(parsed.path, Path::new("/data/output.versatiles"));
	}

	#[test]
	fn test_parse_sftp_url_minimal() {
		let parsed = parse_sftp_url("sftp://myhost/tmp/file.tar").unwrap();
		assert_eq!(parsed.user, None);
		assert_eq!(parsed.password, None);
		assert_eq!(parsed.host, "myhost");
		assert_eq!(parsed.port, 22);
		assert_eq!(parsed.path, Path::new("/tmp/file.tar"));
	}

	#[test]
	fn test_parse_sftp_url_user_no_pass() {
		let parsed = parse_sftp_url("sftp://deploy@server/var/tiles.versatiles").unwrap();
		assert_eq!(parsed.user.as_deref(), Some("deploy"));
		assert_eq!(parsed.password, None);
		assert_eq!(parsed.host, "server");
		assert_eq!(parsed.port, 22);
	}

	#[test]
	fn test_parse_sftp_url_missing_path() {
		assert!(parse_sftp_url("sftp://host").is_err());
	}

	#[test]
	fn test_parse_sftp_url_empty_host() {
		assert!(parse_sftp_url("sftp:///path").is_err());
	}

	#[test]
	fn test_path_from_url() {
		assert_eq!(
			DataWriterSftp::path_from_url("sftp://host/data/out.versatiles"),
			Some(PathBuf::from("/data/out.versatiles"))
		);
	}
}
