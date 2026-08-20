use std::{
	net::{TcpStream, ToSocketAddrs},
	path::{Path, PathBuf},
	sync::{
		Arc, Mutex,
		atomic::{AtomicBool, Ordering},
	},
	thread::JoinHandle,
	time::Duration,
};

use anyhow::{Context, Result, bail};
use reqwest::Url;
use ssh2::Session;

/// A shared SSH session that can be swapped on reconnect — the unit the keepalive
/// pings. `ssh2::Session` is `Clone` and internally `Arc<Mutex<_>>`-guarded, so the
/// keepalive thread and the owning writer can touch it concurrently; ssh2 serializes
/// the actual libssh2 calls.
pub type SharedSession = Arc<Mutex<Session>>;

/// Background keepalive for an SFTP connection.
///
/// Started when an SFTP reader/writer opens its connection; periodically sends an SSH
/// keepalive so the server (and NAT/firewalls) do not reap the session during long idle
/// gaps — e.g. while the writer waits minutes for the next block from a slow source.
/// Stopping is automatic on drop (the idiomatic "close"): the background thread is
/// signalled and joined.
///
/// Best-effort: a failed ping is logged and ignored — the owner's normal retry path
/// reconnects on the next real operation.
pub struct SftpKeepalive {
	stop: Arc<AtomicBool>,
	handle: Option<JoinHandle<()>>,
}

impl SftpKeepalive {
	/// Spawn a keepalive pinging `session` every `VERSATILES_SFTP_KEEPALIVE_SECS`
	/// seconds (default 15). `name` is only used for log messages.
	#[must_use]
	pub fn start(session: SharedSession, name: String) -> Self {
		let secs = u64::from(super::retry::env_u32("VERSATILES_SFTP_KEEPALIVE_SECS", 15));
		let interval = Duration::from_secs(secs.max(1));
		let stop = Arc::new(AtomicBool::new(false));

		let stop_thread = Arc::clone(&stop);
		let handle = std::thread::Builder::new()
			.name("sftp-keepalive".into())
			.spawn(move || {
				// Wake frequently enough to stop promptly, but only ping every `interval`.
				let tick = Duration::from_millis(500).min(interval);
				let mut waited = Duration::ZERO;
				while !stop_thread.load(Ordering::Relaxed) {
					std::thread::sleep(tick);
					waited += tick;
					if waited < interval {
						continue;
					}
					waited = Duration::ZERO;
					if stop_thread.load(Ordering::Relaxed) {
						break;
					}
					// Clone the current session (cheap Arc clone) and release the lock
					// before the (possibly blocking) network call.
					let session = match session.lock() {
						Ok(guard) => guard.clone(),
						Err(_) => break, // poisoned: owner gone
					};
					match session.keepalive_send() {
						Ok(_) => log::trace!("sent SFTP keepalive to '{name}'"),
						Err(e) => log::debug!("SFTP keepalive to '{name}' failed (will reconnect on next op): {e}"),
					}
				}
			})
			.expect("spawning sftp-keepalive thread");

		SftpKeepalive {
			stop,
			handle: Some(handle),
		}
	}
}

impl Drop for SftpKeepalive {
	fn drop(&mut self) {
		self.stop.store(true, Ordering::Relaxed);
		if let Some(handle) = self.handle.take() {
			let _ = handle.join();
		}
	}
}

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
	let connect_timeout = Duration::from_millis(200);
	let tcp = TcpStream::connect_timeout(&addr, connect_timeout)
		.with_context(|| format!("failed to connect to {host}:{port}"))?;

	// Socket-level TCP keepalive so the connection survives idle gaps — e.g. while a
	// large source range is being read and nothing is written for a while — without
	// being reaped by NAT/firewalls or the server. Best-effort: a failure here only
	// makes idle drops more likely, it must not abort the connection. Disabled under
	// `cfg(test)` to match the in-process test server (see SSH keepalive note below).
	#[cfg(not(test))]
	{
		let ka_secs = u64::from(super::retry::env_u32("VERSATILES_SFTP_KEEPALIVE_SECS", 15));
		let keepalive = socket2::TcpKeepalive::new()
			.with_time(Duration::from_secs(ka_secs))
			.with_interval(Duration::from_secs(ka_secs));
		if let Err(e) = socket2::SockRef::from(&tcp).set_tcp_keepalive(&keepalive) {
			log::warn!("failed to enable TCP keepalive on SFTP socket: {e}");
		}
	}

	// SSH handshake
	let mut session = Session::new()?;
	session.set_tcp_stream(tcp);
	// API timeout for individual SFTP operations. Too low and a slow write under load
	// turns into a `Session(-9)` ("API timeout expired") / "draining incoming flow"
	// error mid-transfer; 30 s (configurable) tolerates congested links. In tests
	// the default is 500 ms — enough for the in-memory server while keeping teardown
	// fast (libssh2 blocks for `api_timeout` per dropped handle when the test server
	// never ACKs channel-close). Tests that need more budget pass `?timeout_ms=N`
	// in the URL via `TestSftpServer::with_timeout_ms`.
	#[cfg(not(test))]
	let timeout_ms = super::retry::env_u32("VERSATILES_SFTP_TIMEOUT_MS", 30_000);
	#[cfg(test)]
	let timeout_ms = url
		.query_pairs()
		.find(|(k, _)| k == "timeout_ms")
		.and_then(|(_, v)| v.parse::<u32>().ok())
		.unwrap_or(500);
	session.set_timeout(timeout_ms);
	session.handshake()?;
	// SSH-level keepalive prevents the server's idle disconnect. Disabled in tests
	// because the in-process test server never acknowledges keepalive or channel-close
	// replies, which would make session teardown block for `api_timeout` per drop.
	#[cfg(not(test))]
	session.set_keepalive(true, super::retry::env_u32("VERSATILES_SFTP_KEEPALIVE_SECS", 15));

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
	use std::{fs::File, io::BufReader};

	use ssh2_config::{ParseRule, SshConfig};

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

	#[test]
	fn test_open_session_unresolvable_host() {
		// A hostname that DNS cannot resolve exercises the resolve-error branch
		// around line 30 of open_session.
		let url = Url::parse("sftp://this-host-must-not-exist.invalid:22/path").unwrap();
		let Err(err) = open_session(&url, None) else {
			panic!("expected DNS failure for .invalid TLD");
		};
		let msg = format!("{err:#}");
		assert!(
			msg.contains("resolve") || msg.contains("connect") || msg.contains("not known") || msg.contains("lookup"),
			"expected DNS / connect error, got: {msg}"
		);
	}

	#[rstest::rstest]
	#[case("sftp://host", "")]
	#[case("sftp://host/", "/")]
	#[case("sftp://host/path", "/path")]
	#[case("sftp://user@host:2222", "")]
	#[case("sftp://host/a%20b/file.tar", "/a%20b/file.tar")] // URL-encoded space stays encoded
	fn test_remote_path_variants(#[case] url_str: &str, #[case] expected: &str) {
		let url = Url::parse(url_str).unwrap();
		assert_eq!(remote_path(&url), PathBuf::from(expected));
	}

	#[cfg(all(feature = "ssh2", unix))]
	mod sftp_server_tests {
		use super::*;
		use crate::io::test_sftp_server::TestSftpServer;

		#[tokio::test(flavor = "current_thread")]
		#[serial_test::serial]
		async fn open_session_password_auth() {
			let server = TestSftpServer::start().await;
			let url = server.url("/");
			let session = tokio::task::spawn_blocking(move || open_session(&url, None))
				.await
				.unwrap();
			assert!(session.is_ok(), "expected successful auth: {:?}", session.err());
		}

		#[tokio::test(flavor = "current_thread")]
		#[serial_test::serial]
		async fn open_session_wrong_password() {
			let server = TestSftpServer::start().await;
			let mut url = server.url("/");
			url.set_password(Some("wrongpass")).unwrap();
			let result = tokio::task::spawn_blocking(move || open_session(&url, None))
				.await
				.unwrap();
			assert!(result.is_err(), "expected auth failure with wrong password");
		}

		#[tokio::test(flavor = "current_thread")]
		#[serial_test::serial]
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

		/// Sets the keepalive interval to its one-second floor for the duration of
		/// one test. The default is 15 s, which no test is going to wait out — so
		/// without this the loop only ever reaches its `continue`.
		///
		/// Safe because these tests are `#[serial]`: nothing else is reading the
		/// environment while it is changed.
		struct ShortKeepalive;

		impl ShortKeepalive {
			fn set() -> Self {
				unsafe { std::env::set_var("VERSATILES_SFTP_KEEPALIVE_SECS", "1") };
				ShortKeepalive
			}
		}

		impl Drop for ShortKeepalive {
			fn drop(&mut self) {
				unsafe { std::env::remove_var("VERSATILES_SFTP_KEEPALIVE_SECS") };
			}
		}

		#[tokio::test(flavor = "current_thread")]
		#[serial_test::serial]
		async fn the_keepalive_thread_pings_and_stops_on_drop() {
			// One second is the floor `start` clamps to, and the loop wakes every
			// 500 ms, so a 2.5 s wait sees at least two pings.
			let _interval = ShortKeepalive::set();
			let server = TestSftpServer::start().await;
			let url = server.url("/");
			let session = tokio::task::spawn_blocking(move || open_session(&url, None))
				.await
				.unwrap()
				.expect("test server accepts the password");

			let shared: SharedSession = Arc::new(Mutex::new(session));
			let keepalive = SftpKeepalive::start(Arc::clone(&shared), "test".to_string());

			tokio::time::sleep(Duration::from_millis(2500)).await;

			// Drop signals the thread and joins it; if the loop ignored `stop`,
			// this would hang rather than fail.
			tokio::task::spawn_blocking(move || drop(keepalive)).await.unwrap();
		}

		#[tokio::test(flavor = "current_thread")]
		#[serial_test::serial]
		async fn a_poisoned_session_ends_the_keepalive_rather_than_spinning() {
			let _interval = ShortKeepalive::set();
			let server = TestSftpServer::start().await;
			let url = server.url("/");
			let session = tokio::task::spawn_blocking(move || open_session(&url, None))
				.await
				.unwrap()
				.expect("test server accepts the password");

			let shared: SharedSession = Arc::new(Mutex::new(session));
			let keepalive = SftpKeepalive::start(Arc::clone(&shared), "poisoned".to_string());

			// Poison the mutex: the owner panicked while holding it, so there is
			// nothing left to keep alive.
			let poisoner = Arc::clone(&shared);
			let _ = std::thread::spawn(move || {
				let _guard = poisoner.lock().unwrap();
				panic!("owner died holding the session");
			})
			.join();
			assert!(shared.lock().is_err(), "the mutex should now be poisoned");

			tokio::time::sleep(Duration::from_millis(2500)).await;
			tokio::task::spawn_blocking(move || drop(keepalive)).await.unwrap();
		}

		#[tokio::test(flavor = "current_thread")]
		#[serial_test::serial]
		async fn an_identity_file_that_is_not_a_key_falls_through_to_the_next_method() {
			// The branch where the file exists but libssh2 refuses it, as opposed
			// to the already-covered case of it not existing at all.
			let dir = tempfile::tempdir().unwrap();
			let not_a_key = dir.path().join("id_rubbish");
			std::fs::write(&not_a_key, b"this is not a private key\n").unwrap();

			let server = TestSftpServer::start().await;
			let url = server.url("/");
			let session = tokio::task::spawn_blocking(move || open_session(&url, Some(&not_a_key)))
				.await
				.unwrap();

			assert!(
				session.is_ok(),
				"password auth should still succeed: {:?}",
				session.err()
			);
		}

		#[tokio::test(flavor = "current_thread")]
		#[serial_test::serial]
		async fn without_a_password_every_key_based_method_is_tried_and_reported() {
			// No credentials in the URL, so the agent, ~/.ssh/config and the
			// default key files are each attempted. None of them can authenticate
			// against the test server, and the error names the target rather than
			// whichever method happened to be last.
			let server = TestSftpServer::start().await;
			let mut url = server.url("/");
			url.set_password(None).unwrap();
			url.set_username("nobody").unwrap();
			let target = display_name(&url);

			let result = tokio::task::spawn_blocking(move || open_session(&url, None))
				.await
				.unwrap();

			let error = format!("{:#}", result.err().expect("no key can authenticate here"));
			assert!(error.contains(&target), "should name the target: {error}");
		}

		#[test]
		fn the_agent_reports_when_it_has_nothing_for_this_user() {
			// Exercised directly rather than through open_session: whether a
			// developer machine has an agent running decides which branch runs,
			// and both end in the same place for an unknown user.
			let session = Session::new().unwrap();
			let result = try_agent_auth(&session, "definitely-not-a-real-user");
			assert!(result.is_err(), "an unconnected session cannot authenticate");
		}

		#[test]
		fn the_default_key_files_are_reported_when_none_authenticates() {
			let session = Session::new().unwrap();
			let error = format!(
				"{:#}",
				try_key_auth(&session, "definitely-not-a-real-user").unwrap_err()
			);
			assert!(error.contains(".ssh"), "should name where it looked: {error}");
		}

		#[test]
		fn the_ssh_config_is_reported_when_it_offers_nothing() {
			let session = Session::new().unwrap();
			let error = format!(
				"{:#}",
				try_config_key_auth(&session, "definitely-not-a-real-user", "definitely-not-a-real-host").unwrap_err()
			);
			// Either there is no config at all, or it has no key for this host.
			assert!(
				error.contains(".ssh/config") || error.contains("no suitable SSH key"),
				"{error}"
			);
		}
	}
}
