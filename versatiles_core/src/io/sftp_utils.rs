use std::{
	fmt::Write as _,
	fs::{OpenOptions, create_dir_all},
	io::Write as _,
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
use ssh2::{CheckResult, HashType, HostKeyType, KnownHostFileKind, KnownHostKeyFormat, Session};

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

	// Who the server is, before we tell it who we are: the password branch below
	// would otherwise hand the credentials to whatever answered the connection.
	verify_host_key(&session, host, port)?;
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

/// Check the server's host key against the known-hosts file, accept-new style.
///
/// The three outcomes mirror OpenSSH's `StrictHostKeyChecking=accept-new`:
///
/// - **known and equal** — proceed;
/// - **unknown host** — record the key and proceed, so a first connection is not
///   a dead end;
/// - **known but different** — abort. This is the case worth failing on: either
///   the server was rebuilt, or someone is sitting in the middle of the
///   connection.
///
/// Accept-new leaves exactly one hole — the very first connection to a host,
/// which has nothing to compare against. Closing that hole needs a fingerprint
/// distributed out of band, which this tool has no channel for; everything after
/// that first connection is protected.
///
/// # Errors
/// Returns an error on a mismatched key, and if the known-hosts file cannot be
/// read or written.
fn verify_host_key(session: &Session, host: &str, port: u16) -> Result<()> {
	let Some(path) = known_hosts_path()? else {
		log::warn!("SFTP host key verification is disabled for {host}:{port} (VERSATILES_SFTP_KNOWN_HOSTS)");
		return Ok(());
	};

	// Copied out of the session: the slice points into libssh2's memory, and it
	// is handed to calls on that same session below.
	let (key, key_type) = {
		let (key, key_type) = session.host_key().context("the server presented no host key")?;
		(key.to_vec(), key_type)
	};

	let mut known_hosts = session.known_hosts()?;
	if path.exists() {
		known_hosts
			.read_file(&path, KnownHostFileKind::OpenSSH)
			.with_context(|| format!("failed to read the known hosts file {}", path.display()))?;
	}

	// `check_port` tries `[host]:port` first and then the plain `host`, so an
	// entry written by OpenSSH for the default port still matches.
	match known_hosts.check_port(host, port, &key) {
		CheckResult::Match => {
			log::debug!("SFTP host key for {host}:{port} matches {}", path.display());
			Ok(())
		}
		CheckResult::NotFound => {
			remember_host_key(session, &path, host, port, &key, key_type)
				.with_context(|| format!("failed to record the host key for {host}:{port}"))?;
			log::warn!(
				"SFTP: {host}:{port} was not known; recorded its host key ({}) in {}",
				fingerprint(session),
				path.display()
			);
			Ok(())
		}
		CheckResult::Mismatch => bail!(
			"host key verification failed for {host}:{port}: the server presented {}, which is not the key recorded in {}. \
			 Either the server's key changed, or this connection is being intercepted. \
			 If the change was expected, remove the stale line from that file.",
			fingerprint(session),
			path.display()
		),
		CheckResult::Failure => bail!(
			"host key verification failed for {host}:{port}: the key could not be checked against {}",
			path.display()
		),
	}
}

/// Append one known-hosts line for `host`/`port`.
///
/// Written into a *second*, empty set rather than by rewriting the file: with
/// only this entry in it, `write_string` yields exactly the line to append.
/// `write_file` would instead round-trip every existing entry through libssh2
/// and drop whatever it does not model — `@cert-authority` markers, comments —
/// from a file this tool does not own.
fn remember_host_key(
	session: &Session,
	path: &Path,
	host: &str,
	port: u16,
	key: &[u8],
	key_type: HostKeyType,
) -> Result<()> {
	// OpenSSH's convention: the bracketed form only for a non-default port.
	let name = if port == 22 {
		host.to_owned()
	} else {
		format!("[{host}]:{port}")
	};

	let mut entry = session.known_hosts()?;
	entry.add(&name, key, "added by versatiles", KnownHostKeyFormat::from(key_type))?;
	let hosts = entry.hosts()?;
	let host_entry = hosts.first().context("the host key could not be encoded")?;
	let line = entry.write_string(host_entry, KnownHostFileKind::OpenSSH)?;

	if let Some(dir) = path.parent() {
		create_dir_all(dir).with_context(|| format!("failed to create {}", dir.display()))?;
	}
	// A file whose last line has no terminator would otherwise absorb the new
	// entry into it, corrupting both.
	let needs_newline = match std::fs::read(path) {
		Ok(bytes) => !bytes.is_empty() && !bytes.ends_with(b"\n"),
		Err(_) => false,
	};

	let mut file = OpenOptions::new()
		.create(true)
		.append(true)
		.open(path)
		.with_context(|| format!("failed to open {} for appending", path.display()))?;
	if needs_newline {
		writeln!(file)?;
	}
	writeln!(file, "{}", line.trim_end())?;

	Ok(())
}

/// The known-hosts file accept-new reads and appends to.
///
/// `VERSATILES_SFTP_KNOWN_HOSTS` overrides the path; setting it to `off` (or
/// `none`, or nothing) disables verification, which is the escape hatch for
/// throwaway hosts whose key changes on every boot. Under `cfg(test)` the
/// default is "disabled" so that running the suite never reads or writes the
/// developer's own `~/.ssh/known_hosts`; the tests that exercise this code set
/// the variable to a temporary file.
fn known_hosts_path() -> Result<Option<PathBuf>> {
	if let Ok(value) = std::env::var("VERSATILES_SFTP_KNOWN_HOSTS") {
		let value = value.trim();
		if value.is_empty() || value.eq_ignore_ascii_case("off") || value.eq_ignore_ascii_case("none") {
			return Ok(None);
		}
		return Ok(Some(PathBuf::from(value)));
	}

	#[cfg(test)]
	{
		Ok(None)
	}
	#[cfg(not(test))]
	{
		let home = dirs_home().context(
			"could not determine the home directory holding ~/.ssh/known_hosts, so the SFTP host key cannot be verified. \
			 Set VERSATILES_SFTP_KNOWN_HOSTS to a file path, or to 'off' to connect without verifying",
		)?;
		Ok(Some(home.join(".ssh").join("known_hosts")))
	}
}

/// The server's SHA-256 host key fingerprint, for log and error messages.
///
/// Hex rather than OpenSSH's base64 spelling: it needs no dependency, and these
/// strings are read by a human comparing two values, not parsed.
fn fingerprint(session: &Session) -> String {
	session.host_key_hash(HashType::Sha256).map_or_else(
		|| "an unknown host key".to_owned(),
		|hash| {
			let mut out = String::from("SHA256(hex):");
			for byte in hash {
				// Infallible for a String; the trait method returns a Result anyway.
				let _ = write!(out, "{byte:02x}");
			}
			out
		},
	)
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

	/// Sets `VERSATILES_SFTP_KNOWN_HOSTS` for the duration of a test and restores
	/// the previous value on drop.
	///
	/// `set_var` is unsafe in edition 2024 because another thread may be reading
	/// the environment concurrently. Every test using this guard is marked
	/// `#[serial_test::serial]`, so no other test is running, and nothing else in
	/// the process reads this variable outside `known_hosts_path`.
	struct KnownHostsEnv(Option<String>);

	impl KnownHostsEnv {
		fn set(value: &str) -> Self {
			let previous = std::env::var("VERSATILES_SFTP_KNOWN_HOSTS").ok();
			unsafe { std::env::set_var("VERSATILES_SFTP_KNOWN_HOSTS", value) };
			Self(previous)
		}
	}

	impl Drop for KnownHostsEnv {
		fn drop(&mut self) {
			match self.0.take() {
				Some(value) => unsafe { std::env::set_var("VERSATILES_SFTP_KNOWN_HOSTS", value) },
				None => unsafe { std::env::remove_var("VERSATILES_SFTP_KNOWN_HOSTS") },
			}
		}
	}

	/// `off`, `none` and an empty value all mean "do not verify"; anything else
	/// is a path.
	#[test]
	#[serial_test::serial]
	fn known_hosts_path_from_env() {
		{
			let _env = KnownHostsEnv::set("off");
			assert_eq!(known_hosts_path().unwrap(), None);
		}
		{
			let _env = KnownHostsEnv::set("NONE");
			assert_eq!(known_hosts_path().unwrap(), None);
		}
		{
			let _env = KnownHostsEnv::set("  ");
			assert_eq!(known_hosts_path().unwrap(), None);
		}
		{
			let _env = KnownHostsEnv::set("/tmp/some/known_hosts");
			assert_eq!(
				known_hosts_path().unwrap(),
				Some(PathBuf::from("/tmp/some/known_hosts"))
			);
		}
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

		/// Accept-new: an unknown host connects and its key is written down; the
		/// next connection to the same host matches that record instead of
		/// adding a second one.
		#[tokio::test(flavor = "current_thread")]
		#[serial_test::serial]
		async fn known_hosts_accept_new_records_the_key() {
			let dir = tempfile::tempdir().unwrap();
			let path = dir.path().join("known_hosts");
			let _env = KnownHostsEnv::set(path.to_str().unwrap());

			// An existing file whose last line has no terminator: the new entry
			// must not be appended onto it.
			std::fs::write(&path, b"# an existing entry without a trailing newline").unwrap();

			let server = TestSftpServer::start().await;
			let url = server.url("/");
			let port = url.port().unwrap();

			let first = tokio::task::spawn_blocking({
				let url = url.clone();
				move || open_session(&url, None)
			})
			.await
			.unwrap();
			assert!(first.is_ok(), "first connection failed: {:?}", first.err());

			let recorded = std::fs::read_to_string(&path).unwrap();
			assert!(
				recorded.contains(&format!("[127.0.0.1]:{port}")),
				"host key was not recorded: {recorded:?}"
			);

			let second = tokio::task::spawn_blocking(move || open_session(&url, None))
				.await
				.unwrap();
			assert!(second.is_ok(), "second connection failed: {:?}", second.err());

			let contents = std::fs::read_to_string(&path).unwrap();
			let lines = contents
				.lines()
				.filter(|line| !line.trim().is_empty())
				.collect::<Vec<_>>();
			assert_eq!(
				lines.len(),
				2,
				"expected the pre-existing line plus one recorded key, got {lines:?}"
			);
			assert_eq!(lines[0], "# an existing entry without a trailing newline");
		}

		/// A recorded key that does not match the one the server presents aborts
		/// the connection — the case accept-new exists to catch.
		///
		/// Built from two test servers, each of which generates its own Ed25519
		/// key: the entry recorded for the first is repointed at the second's
		/// port, so the key material is genuine and only the host association is
		/// wrong.
		#[tokio::test(flavor = "current_thread")]
		#[serial_test::serial]
		async fn known_hosts_mismatch_is_refused() {
			let dir = tempfile::tempdir().unwrap();
			let path = dir.path().join("known_hosts");
			let _env = KnownHostsEnv::set(path.to_str().unwrap());

			let server_a = TestSftpServer::start().await;
			let url_a = server_a.url("/");
			let port_a = url_a.port().unwrap();
			let recorded = tokio::task::spawn_blocking(move || open_session(&url_a, None))
				.await
				.unwrap();
			assert!(recorded.is_ok(), "could not record server A: {:?}", recorded.err());

			let server_b = TestSftpServer::start().await;
			let url_b = server_b.url("/");
			let port_b = url_b.port().unwrap();
			assert_ne!(port_a, port_b, "the two test servers reused a port");

			let entry = std::fs::read_to_string(&path)
				.unwrap()
				.replace(&format!("[127.0.0.1]:{port_a}"), &format!("[127.0.0.1]:{port_b}"));
			std::fs::write(&path, entry).unwrap();

			let result = tokio::task::spawn_blocking(move || open_session(&url_b, None))
				.await
				.unwrap();
			// `ssh2::Session` is not `Debug`, so `expect_err` is not available.
			let error = match result {
				Ok(_) => panic!("a mismatched host key must not connect"),
				Err(error) => format!("{error:#}"),
			};
			assert!(
				error.contains("host key verification failed"),
				"unexpected error: {error}"
			);
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
