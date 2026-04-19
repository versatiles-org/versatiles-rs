use anyhow::{Context, Result, bail};
use reqwest::Url;
use ssh2::Session;
use std::{
	net::{TcpStream, ToSocketAddrs},
	path::{Path, PathBuf},
	time::Duration,
};

/// Opens an authenticated SSH session from an SFTP URL.
///
/// # Authentication priority
/// 1. Credentials in URL (password auth)
/// 2. Explicit identity file (if provided)
/// 3. SSH agent
/// 4. `~/.ssh/config` `IdentityFile` for the target host
/// 5. Default key files (~/.ssh/id_ed25519, id_rsa, id_ecdsa)
pub fn open_session(url: &Url, identity_file: Option<&Path>) -> Result<Session> {
	let host = url.host_str().context("SFTP URL has no host")?;
	let port = url.port().unwrap_or(22);
	let username = if url.username().is_empty() {
		"root"
	} else {
		url.username()
	};

	// Connect TCP with timeout
	let addr = (host, port)
		.to_socket_addrs()
		.with_context(|| format!("failed to resolve {host}:{port}"))?
		.next()
		.with_context(|| format!("no addresses found for {host}:{port}"))?;
	// Use a short timeout in tests so unreachable-host tests complete in milliseconds.
	#[cfg(not(test))]
	let connect_timeout = Duration::from_secs(30);
	#[cfg(test)]
	let connect_timeout = Duration::from_secs(1);
	let tcp = TcpStream::connect_timeout(&addr, connect_timeout)
		.with_context(|| format!("failed to connect to {host}:{port}"))?;

	// SSH handshake
	let mut session = Session::new()?;
	session.set_tcp_stream(tcp);
	// Use a shorter timeout in tests so session teardown completes in ~5s instead of ~30s.
	#[cfg(not(test))]
	session.set_timeout(10_000);
	#[cfg(test)]
	session.set_timeout(3_000);
	session.handshake()?;
	// Keepalive causes session teardown to block for `api_timeout` per drop in tests
	// because the test server never acknowledges keepalive or channel-close replies.
	#[cfg(not(test))]
	session.set_keepalive(true, 60);

	// Sanitized target for log messages (no credentials)
	let target = display_name(url);

	// Authenticate — try methods in priority order, stop on first success
	let password = url.password();
	if let Some(password) = password {
		log::debug!("SFTP auth: trying password for {target}");
		if session.userauth_password(username, password).is_ok() && session.authenticated() {
			log::debug!("SFTP auth: password succeeded");
			return Ok(session);
		}
		log::debug!("SFTP auth: password failed");
	}

	if let Some(identity) = identity_file {
		log::debug!("SFTP auth: trying identity file {identity:?} for {target}");
		if identity.exists() {
			match session.userauth_pubkey_file(username, None, identity, None) {
				Ok(()) if session.authenticated() => {
					log::debug!("SFTP auth: identity file succeeded");
					return Ok(session);
				}
				Ok(()) => log::debug!("SFTP auth: identity file returned Ok but not authenticated"),
				Err(e) => log::debug!("SFTP auth: identity file failed: {e}"),
			}
		} else {
			log::debug!("SFTP auth: identity file {identity:?} does not exist");
		}
	}

	log::debug!("SFTP auth: trying SSH agent for {target}");
	if try_agent_auth(&session, username).is_ok() && session.authenticated() {
		log::debug!("SFTP auth: agent succeeded");
		return Ok(session);
	}
	log::debug!("SFTP auth: agent failed");

	log::debug!("SFTP auth: trying ~/.ssh/config keys for {target}");
	if try_config_key_auth(&session, username, host).is_ok() && session.authenticated() {
		log::debug!("SFTP auth: config key succeeded");
		return Ok(session);
	}
	log::debug!("SFTP auth: config key failed");

	log::debug!("SFTP auth: trying default key files for {target}");
	try_key_auth(&session, username).with_context(|| format!("all authentication methods failed for {target}"))?;

	if !session.authenticated() {
		bail!("SSH authentication failed for {target}");
	}

	Ok(session)
}

/// Extract the remote file path from an SFTP URL.
#[must_use]
pub fn remote_path(url: &Url) -> PathBuf {
	PathBuf::from(url.path())
}

/// Build a sanitized display name (without credentials).
#[must_use]
pub fn display_name(url: &Url) -> String {
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

/// Try authenticating with identity files from `~/.ssh/config`.
fn try_config_key_auth(session: &Session, username: &str, host: &str) -> Result<()> {
	use ssh2_config::{ParseRule, SshConfig};
	use std::fs::File;
	use std::io::BufReader;

	let home = dirs_home()?;
	let config_path = home.join(".ssh/config");
	if !config_path.exists() {
		bail!("no ~/.ssh/config found");
	}

	let file = File::open(&config_path).with_context(|| format!("failed to open {config_path:?}"))?;
	let mut reader = BufReader::new(file);
	let config = SshConfig::default().parse(&mut reader, ParseRule::ALLOW_UNKNOWN_FIELDS)?;

	let params = config.query(host);
	let identity_files = params.identity_file.unwrap_or_default();

	for identity in &identity_files {
		// Expand ~ in paths
		let expanded = if identity.starts_with("~") {
			home.join(identity.strip_prefix("~").unwrap_or(identity))
		} else {
			identity.clone()
		};
		if expanded.exists() && session.userauth_pubkey_file(username, None, &expanded, None).is_ok() {
			return Ok(());
		}
	}
	bail!("no suitable SSH key found in ~/.ssh/config for {host}")
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
	fn test_remote_path_root() {
		let url = Url::parse("sftp://host/").unwrap();
		assert_eq!(remote_path(&url), PathBuf::from("/"));
	}

	#[test]
	fn test_remote_path_nested() {
		let url = Url::parse("sftp://host/a/b/c/d/file.tar").unwrap();
		assert_eq!(remote_path(&url), PathBuf::from("/a/b/c/d/file.tar"));
	}

	#[test]
	fn test_remote_path_with_credentials() {
		let url = Url::parse("sftp://user:pass@host/data/file.versatiles").unwrap();
		assert_eq!(remote_path(&url), PathBuf::from("/data/file.versatiles"));
	}

	#[test]
	fn test_remote_path_with_port() {
		let url = Url::parse("sftp://host:2222/data/file.versatiles").unwrap();
		assert_eq!(remote_path(&url), PathBuf::from("/data/file.versatiles"));
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

	#[test]
	fn test_display_name_custom_port() {
		let url = Url::parse("sftp://host:9922/file.tar").unwrap();
		assert_eq!(display_name(&url), "sftp://host:9922/file.tar");
	}

	#[test]
	fn test_display_name_username_only() {
		let url = Url::parse("sftp://admin@host/path").unwrap();
		// Should strip the username too
		assert_eq!(display_name(&url), "sftp://host:22/path");
	}

	#[test]
	fn test_display_name_no_path() {
		let url = Url::parse("sftp://host").unwrap();
		assert_eq!(display_name(&url), "sftp://host:22");
	}

	#[test]
	fn test_home_dir_returns_some() {
		// HOME (unix) or USERPROFILE (windows) should be set in CI and dev
		assert!(home_dir().is_some());
	}

	#[test]
	fn test_dirs_home_returns_ok() {
		assert!(dirs_home().is_ok());
	}

	#[test]
	fn test_open_session_missing_host() {
		// A URL with no host should fail
		let url = Url::parse("sftp:///path/file").unwrap();
		let result = open_session(&url, None);
		let err = result.err().expect("expected error for missing host");
		assert!(err.to_string().contains("no host"));
	}

	#[test]
	fn test_open_session_unreachable_host() {
		// Connection to a non-routable IP should fail with a TCP error
		let url = Url::parse("sftp://192.0.2.1:22222/path").unwrap();
		let result = open_session(&url, None);
		assert!(result.is_err());
	}

	#[cfg(all(feature = "ssh2", unix))]
	mod sftp_server_tests {
		use super::*;
		use crate::io::test_sftp_server::TestSftpServer;

		#[tokio::test(flavor = "multi_thread")]
		async fn open_session_password_auth() {
			let server = TestSftpServer::start().await;
			let url = server.url("/");
			let session = tokio::task::spawn_blocking(move || open_session(&url, None))
				.await
				.unwrap();
			assert!(session.is_ok(), "expected successful auth: {:?}", session.err());
		}

		#[tokio::test(flavor = "multi_thread")]
		async fn open_session_wrong_password() {
			let server = TestSftpServer::start().await;
			let mut url = server.url("/");
			url.set_password(Some("wrongpass")).unwrap();
			let result = tokio::task::spawn_blocking(move || open_session(&url, None))
				.await
				.unwrap();
			assert!(result.is_err(), "expected auth failure with wrong password");
		}

		#[tokio::test(flavor = "multi_thread")]
		async fn open_session_with_unused_identity_file() {
			let server = TestSftpServer::start().await;
			let url = server.url("/");
			let session =
				tokio::task::spawn_blocking(move || open_session(&url, Some(std::path::Path::new("/nonexistent/key"))))
					.await
					.unwrap();
			assert!(
				session.is_ok(),
				"password auth should succeed even with a missing identity file"
			);
		}
	}
}
