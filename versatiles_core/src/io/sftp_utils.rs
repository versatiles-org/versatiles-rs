use anyhow::{Context, Result, bail};
use reqwest::Url;
use ssh2::Session;
use std::{net::TcpStream, path::PathBuf};

/// Opens an authenticated SSH session from an SFTP URL.
///
/// # Authentication priority
/// 1. Credentials in URL (password auth)
/// 2. SSH agent
/// 3. Default key files (~/.ssh/id_ed25519, id_rsa, id_ecdsa)
pub(super) fn open_session(url: &Url) -> Result<Session> {
	let host = url.host_str().context("SFTP URL has no host")?;
	let port = url.port().unwrap_or(22);
	let username = if url.username().is_empty() {
		"root"
	} else {
		url.username()
	};

	// Connect TCP
	let tcp = TcpStream::connect((host, port)).with_context(|| format!("failed to connect to {host}:{port}"))?;

	// SSH handshake
	let mut session = Session::new()?;
	session.set_tcp_stream(tcp);
	session.handshake()?;

	// Authenticate
	let password = url.password();
	let auth_result = if let Some(password) = password {
		session.userauth_password(username, password)
	} else {
		Err(ssh2::Error::from_errno(ssh2::ErrorCode::Session(-1)))
	};

	if auth_result.is_err() {
		let agent_result = try_agent_auth(&session, username);
		if agent_result.is_err() {
			try_key_auth(&session, username)
				.with_context(|| format!("all authentication methods failed for {username}@{host}:{port}"))?;
		}
	}

	if !session.authenticated() {
		bail!("SSH authentication failed for {username}@{host}:{port}");
	}

	Ok(session)
}

/// Extract the remote file path from an SFTP URL.
pub(super) fn remote_path(url: &Url) -> PathBuf {
	PathBuf::from(url.path())
}

/// Build a sanitized display name (without credentials).
pub(super) fn display_name(url: &Url) -> String {
	let host = url.host_str().unwrap_or("unknown");
	let port = url.port().unwrap_or(22);
	format!("sftp://{host}:{port}{}", url.path())
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

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_remote_path() {
		let url = Url::parse("sftp://host/data/tiles.versatiles").unwrap();
		assert_eq!(remote_path(&url), PathBuf::from("/data/tiles.versatiles"));
	}

	#[test]
	fn test_display_name_strips_credentials() {
		let url = Url::parse("sftp://user:secret@host:2222/data/tiles.versatiles").unwrap();
		assert_eq!(display_name(&url), "sftp://host:2222/data/tiles.versatiles");
	}

	#[test]
	fn test_display_name_default_port() {
		let url = Url::parse("sftp://host/path/file.tar").unwrap();
		assert_eq!(display_name(&url), "sftp://host:22/path/file.tar");
	}
}
