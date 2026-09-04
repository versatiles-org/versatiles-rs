use std::{
	net::{TcpStream as StdTcpStream, ToSocketAddrs},
	path::{Path, PathBuf},
	sync::Arc,
	time::Duration,
};

use anyhow::{Context, Result, anyhow, bail};
use reqwest::Url;
use russh::{
	client::{self, Config, Handle},
	keys::{HashAlg, PrivateKeyWithHashAlg, PublicKeyOrCertificate, known_hosts, ssh_key},
};

/// An authenticated SSH connection.
///
/// Unlike the `ssh2::Session` this replaces, the handle is already internally
/// shareable: every operation goes through an mpsc channel to one background
/// task that owns the transport, so clones are cheap and concurrent use needs
/// no `Mutex` of ours. What used to be `Arc<Mutex<Session>>` is therefore just
/// this type.
pub type SshHandle = Handle<ClientHandler>;

/// An opened SFTP subsystem, ready for file operations.
pub type Sftp = russh_sftp::client::SftpSession;

/// Opens an authenticated SSH session from an SFTP URL.
///
/// # Authentication priority
/// 1. Credentials in URL (password auth)
/// 2. Explicit identity file (if provided)
/// 3. SSH agent
/// 4. `~/.ssh/config` `IdentityFile` for the target host
/// 5. Default key files (~/.ssh/id_ed25519, id_rsa, id_ecdsa)
///
/// # Errors
/// Returns an error if the host cannot be resolved or reached, if host key
/// verification fails, or if no authentication method succeeds.
pub async fn open_session(url: &Url, identity_file: Option<&Path>) -> Result<SshHandle> {
	let host = url.host_str().context("SFTP URL has no host")?;
	let port = url.port().unwrap_or(22);
	let username = if url.username().is_empty() {
		"root"
	} else {
		url.username()
	};

	let timeout = operation_timeout(url);
	let tcp = connect_tcp(host, port)?;

	// Budget for the handshake as a whole. Under ssh2 this needed `SO_RCVTIMEO`
	// on the socket, because libssh2 read from a blocking socket and only
	// consulted its own timeout *after* `recv` returned — so a server that
	// accepted the connection and then went silent parked the process in
	// `recvfrom` indefinitely (measured at over an hour). An async transport has
	// no such hole: nothing blocks a thread, and one `timeout` around the future
	// bounds the whole exchange.
	let handler = ClientHandler {
		host: host.to_owned(),
		port,
	};
	let handle = with_timeout(
		timeout,
		client::connect_stream(Arc::new(client_config(timeout)), tcp, handler),
		|| format!("SSH handshake with {host}:{port} timed out"),
	)
	.await?
	.with_context(|| format!("SSH handshake with {host}:{port} failed"))?;

	// Host key verification already happened inside the handshake above:
	// `ClientHandler::check_server_key` runs before any authentication packet is
	// sent, so the password branch below cannot hand credentials to whatever
	// answered the connection.
	authenticate(handle, url, username, identity_file, timeout).await
}

/// Opens the SFTP subsystem on an authenticated session.
///
/// # Errors
/// Returns an error if the channel cannot be opened or the server does not
/// offer the `sftp` subsystem.
pub async fn open_sftp(handle: &SshHandle) -> Result<Sftp> {
	let channel = handle
		.channel_open_session()
		.await
		.context("failed to open an SSH channel")?;
	channel
		.request_subsystem(true, "sftp")
		.await
		.context("the server refused the SFTP subsystem")?;
	Sftp::new(channel.into_stream())
		.await
		.context("failed to start an SFTP session")
}

/// Extract the remote file path from an SFTP URL.
///
/// A `String`, not a `PathBuf`: SFTP paths are always `/`-separated, and now
/// that Windows is a supported build target `PathBuf::join` would splice a
/// backslash into a remote path. Under ssh2 this could not happen because the
/// whole SFTP stack was unix-only.
#[must_use]
pub fn remote_path(url: &Url) -> String {
	url.path().to_owned()
}

/// Join a relative path onto a remote directory, `/`-separated.
#[must_use]
pub fn remote_join(base: &str, relative: &str) -> String {
	let base = base.trim_end_matches('/');
	let relative = relative.trim_start_matches('/');
	if relative.is_empty() {
		return if base.is_empty() {
			"/".to_owned()
		} else {
			base.to_owned()
		};
	}
	format!("{base}/{relative}")
}

/// The parent directory of a remote path, or `None` at the root.
#[must_use]
pub fn remote_parent(path: &str) -> Option<&str> {
	let trimmed = path.trim_end_matches('/');
	match trimmed.rfind('/') {
		None | Some(0) => None,
		Some(index) => Some(&trimmed[..index]),
	}
}

/// Build a sanitized display name (without credentials).
#[must_use]
pub fn display_name(url: &Url) -> String {
	let host = url.host_str().unwrap_or("unknown");
	let port = url.port().unwrap_or(22);
	format!("sftp://{host}:{port}{}", url.path())
}

// ---------------------------------------------------------------------------
// Connection setup
// ---------------------------------------------------------------------------

/// The per-operation budget, in the same shape the ssh2 implementation used.
///
/// In tests the default is 500 ms — enough for the in-process server while
/// keeping teardown fast. Tests that need more budget pass `?timeout_ms=N` in
/// the URL via `TestSftpServer::with_timeout_ms`.
fn operation_timeout(url: &Url) -> Option<Duration> {
	#[cfg(not(test))]
	let ms = super::retry::env_u32("VERSATILES_SFTP_TIMEOUT_MS", 30_000);
	#[cfg(test)]
	let ms = url
		.query_pairs()
		.find(|(k, _)| k == "timeout_ms")
		.and_then(|(_, v)| v.parse::<u32>().ok())
		.unwrap_or(500);

	// `0` keeps the historical meaning: no timeout at all.
	#[cfg(not(test))]
	let _ = url;
	(ms > 0).then(|| Duration::from_millis(u64::from(ms)))
}

/// Apply `timeout` to `future`, turning expiry into an error from `message`.
async fn with_timeout<F: Future>(
	timeout: Option<Duration>,
	future: F,
	message: impl FnOnce() -> String,
) -> Result<F::Output> {
	match timeout {
		Some(limit) => tokio::time::timeout(limit, future)
			.await
			.map_err(|_| anyhow!(message())),
		None => Ok(future.await),
	}
}

/// How often to send an SSH keepalive, from `VERSATILES_SFTP_KEEPALIVE_SECS`.
///
/// Clamped to at least one second: a zero interval would ask russh to send a
/// keepalive on every poll.
fn keepalive_interval() -> Duration {
	let secs = u64::from(super::retry::env_u32("VERSATILES_SFTP_KEEPALIVE_SECS", 15));
	Duration::from_secs(secs.max(1))
}

/// The russh client configuration.
///
/// `keepalive_interval` replaces the background thread the ssh2 implementation
/// had to run: russh sends the keepalive from the task that already owns the
/// transport, so there is no second handle to the session to synchronise and
/// nothing to join on drop.
fn client_config(timeout: Option<Duration>) -> Config {
	Config {
		// Only under `cfg(not(test))`: the in-process test server never answers
		// keepalives, so enabling them would tear down every test session.
		keepalive_interval: (!cfg!(test)).then(keepalive_interval),
		keepalive_max: 3,
		inactivity_timeout: timeout,
		..Config::default()
	}
}

/// Open the TCP connection the SSH transport runs over.
fn connect_tcp(host: &str, port: u16) -> Result<tokio::net::TcpStream> {
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

	// `connect_timeout` is the one part of setup with no async equivalent that
	// also bounds *connect* itself, so the socket is built synchronously and
	// handed to tokio afterwards.
	let tcp = StdTcpStream::connect_timeout(&addr, connect_timeout)
		.with_context(|| format!("failed to connect to {host}:{port}"))?;

	// Socket-level TCP keepalive so the connection survives idle gaps — e.g.
	// while a large source range is being read and nothing is written for a
	// while — without being reaped by NAT/firewalls or the server. Best-effort:
	// a failure here only makes idle drops more likely, it must not abort the
	// connection.
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

	tcp.set_nonblocking(true)
		.with_context(|| format!("failed to put the socket to {host}:{port} into non-blocking mode"))?;
	tokio::net::TcpStream::from_std(tcp).with_context(|| format!("failed to register the socket to {host}:{port}"))
}

// ---------------------------------------------------------------------------
// Authentication
// ---------------------------------------------------------------------------

/// Try each authentication method in priority order, stopping on first success.
async fn authenticate(
	mut handle: SshHandle,
	url: &Url,
	username: &str,
	identity_file: Option<&Path>,
	timeout: Option<Duration>,
) -> Result<SshHandle> {
	// Sanitized target for log messages (no credentials)
	let target = display_name(url);
	let host = url.host_str().unwrap_or("unknown");

	if let Some(password) = url.password() {
		log::debug!("SFTP auth: trying password for {target}");
		let result = with_timeout(timeout, handle.authenticate_password(username, password), || {
			format!("password authentication with {target} timed out")
		})
		.await?;
		match result {
			Ok(r) if r.success() => {
				log::debug!("SFTP auth: password succeeded");
				return Ok(handle);
			}
			Ok(_) => log::debug!("SFTP auth: password failed"),
			Err(e) => log::debug!("SFTP auth: password failed: {e}"),
		}
	}

	if let Some(identity) = identity_file {
		log::debug!("SFTP auth: trying identity file {identity:?} for {target}");
		if identity.exists() {
			if try_key_file(&mut handle, username, identity).await {
				log::debug!("SFTP auth: identity file succeeded");
				return Ok(handle);
			}
			log::debug!("SFTP auth: identity file failed");
		} else {
			log::debug!("SFTP auth: identity file {identity:?} does not exist");
		}
	}

	log::debug!("SFTP auth: trying SSH agent for {target}");
	match try_agent_auth(&mut handle, username).await {
		Ok(true) => {
			log::debug!("SFTP auth: agent succeeded");
			return Ok(handle);
		}
		Ok(false) => log::debug!("SFTP auth: agent has no suitable identity"),
		Err(e) => log::debug!("SFTP auth: agent failed: {e}"),
	}

	log::debug!("SFTP auth: trying ~/.ssh/config keys for {target}");
	match try_config_key_auth(&mut handle, username, host).await {
		Ok(true) => {
			log::debug!("SFTP auth: config key succeeded");
			return Ok(handle);
		}
		Ok(false) => log::debug!("SFTP auth: no ~/.ssh/config key matched"),
		Err(e) => log::debug!("SFTP auth: config key failed: {e}"),
	}

	log::debug!("SFTP auth: trying default key files for {target}");
	if try_default_key_auth(&mut handle, username).await? {
		return Ok(handle);
	}

	bail!("all authentication methods failed for {target}")
}

/// Offer one on-disk private key. Returns whether the server accepted it.
///
/// Unreadable or passphrase-protected keys are a `debug` line and a `false`,
/// not an error: the caller has further methods to try, and a key that cannot
/// be loaded is indistinguishable from one the server would have rejected.
async fn try_key_file(handle: &mut SshHandle, username: &str, path: &Path) -> bool {
	let key = match russh::keys::load_secret_key(path, None) {
		Ok(key) => key,
		Err(e) => {
			log::debug!("SFTP auth: cannot load key {path:?}: {e}");
			return false;
		}
	};

	// `None` lets russh pick the signature algorithm; for RSA keys it negotiates
	// rsa-sha2-512/256 rather than the SHA-1 variant modern servers refuse.
	let key = PrivateKeyWithHashAlg::new(Arc::new(key), Some(HashAlg::Sha512));
	match handle.authenticate_publickey(username, key).await {
		Ok(result) => result.success(),
		Err(e) => {
			log::debug!("SFTP auth: key {path:?} rejected: {e}");
			false
		}
	}
}

/// Connect to the local SSH agent.
///
/// The agent is reached over a Unix socket on unix and, on Windows, over the
/// named pipe OpenSSH's own agent listens on — the platform difference libssh2
/// used to hide, and the only place where this migration has to spell it out.
///
/// Split out from [`try_agent_auth`] so the "is there an agent at all" half can
/// be tested without a live session, which an `ssh2::Session` allowed only by
/// handing the auth helpers an unconnected session.
#[cfg(unix)]
async fn connect_agent() -> Result<russh::keys::agent::client::AgentClient<tokio::net::UnixStream>> {
	russh::keys::agent::client::AgentClient::connect_env()
		.await
		.context("no SSH agent available")
}

#[cfg(windows)]
async fn connect_agent()
-> Result<russh::keys::agent::client::AgentClient<tokio::net::windows::named_pipe::NamedPipeClient>> {
	russh::keys::agent::client::AgentClient::connect_named_pipe(r"\\.\pipe\openssh-ssh-agent")
		.await
		.context("no SSH agent available")
}

/// Try authenticating with the SSH agent.
#[cfg(any(unix, windows))]
async fn try_agent_auth(handle: &mut SshHandle, username: &str) -> Result<bool> {
	let mut agent = connect_agent().await?;
	let identities = agent
		.request_identities()
		.await
		.context("the SSH agent refused to list its identities")?;

	for identity in identities {
		let russh::keys::agent::AgentIdentity::PublicKey { key, .. } = identity else {
			continue;
		};
		if handle
			.authenticate_publickey_with(username, key, Some(HashAlg::Sha512), &mut agent)
			.await
			.is_ok_and(|result| result.success())
		{
			return Ok(true);
		}
	}
	Ok(false)
}

#[cfg(not(any(unix, windows)))]
async fn try_agent_auth(_handle: &mut SshHandle, _username: &str) -> Result<bool> {
	bail!("SSH agents are not supported on this platform")
}

/// The identity files `~/.ssh/config` names for `host`, `~` expanded, filtered
/// down to the ones that exist.
fn config_identity_files(host: &str) -> Result<Vec<PathBuf>> {
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

	Ok(config
		.query(host)
		.identity_file
		.unwrap_or_default()
		.iter()
		.map(|identity| {
			if identity.starts_with("~") {
				home.join(identity.strip_prefix("~").unwrap_or(identity))
			} else {
				identity.clone()
			}
		})
		.filter(|path| path.exists())
		.collect())
}

/// Try authenticating with identity files from `~/.ssh/config`.
async fn try_config_key_auth(handle: &mut SshHandle, username: &str, host: &str) -> Result<bool> {
	for identity in config_identity_files(host)? {
		if try_key_file(handle, username, &identity).await {
			return Ok(true);
		}
	}
	Ok(false)
}

/// The default key files, filtered down to the ones that exist.
fn default_key_files() -> Result<Vec<PathBuf>> {
	let home = dirs_home()?;
	Ok([".ssh/id_ed25519", ".ssh/id_rsa", ".ssh/id_ecdsa"]
		.into_iter()
		.map(|name| home.join(name))
		.filter(|path| path.exists())
		.collect())
}

/// Try authenticating with default key files.
async fn try_default_key_auth(handle: &mut SshHandle, username: &str) -> Result<bool> {
	for key_path in default_key_files()? {
		if try_key_file(handle, username, &key_path).await {
			return Ok(true);
		}
	}
	Ok(false)
}

// ---------------------------------------------------------------------------
// Host key verification
// ---------------------------------------------------------------------------

/// Carries the host identity into russh's handshake callback.
pub struct ClientHandler {
	host: String,
	port: u16,
}

impl client::Handler for ClientHandler {
	type Error = anyhow::Error;

	// Known-hosts checking is file I/O, not network I/O, so nothing here awaits.
	// The signature is russh's, and it is called from inside the handshake.
	#[allow(
		clippy::unused_async_trait_impl,
		reason = "the async signature is required by russh's Handler trait"
	)]
	async fn check_server_key(&mut self, key: &PublicKeyOrCertificate) -> Result<bool> {
		let key = match key {
			PublicKeyOrCertificate::PublicKey { key, .. } => key,
			// The known-hosts handling below models plain keys only. Accepting a
			// certificate would mean validating it against a CA line this code
			// does not read, so it is refused rather than trusted blindly.
			PublicKeyOrCertificate::Certificate(_) => bail!(
				"host key verification failed for {}:{}: the server presented an OpenSSH certificate, \
				 which versatiles does not verify",
				self.host,
				self.port
			),
		};
		verify_host_key(key, &self.host, self.port)?;
		Ok(true)
	}
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
fn verify_host_key(key: &ssh_key::PublicKey, host: &str, port: u16) -> Result<()> {
	let Some(path) = known_hosts_path()? else {
		log::warn!("SFTP host key verification is disabled for {host}:{port} (VERSATILES_SFTP_KNOWN_HOSTS)");
		return Ok(());
	};

	// russh reports the three outcomes as `Ok(true)` / `Ok(false)` / a
	// `KeyChanged` error, and its file handling already matches what this code
	// used to do by hand: entries are *appended* (never round-tripped, so
	// `@cert-authority` lines and comments in a file versatiles does not own
	// survive), a missing trailing newline is repaired first, and the bracketed
	// `[host]:port` form is used only for a non-default port.
	match known_hosts::check_known_hosts_path(host, port, key, &path) {
		Ok(true) => {
			log::debug!("SFTP host key for {host}:{port} matches {}", path.display());
			Ok(())
		}
		Ok(false) => {
			known_hosts::learn_known_hosts_path(host, port, key, &path)
				.with_context(|| format!("failed to record the host key for {host}:{port}"))?;
			log::warn!(
				"SFTP: {host}:{port} was not known; recorded its host key ({}) in {}",
				fingerprint(key),
				path.display()
			);
			Ok(())
		}
		Err(russh::keys::Error::KeyChanged { line }) => bail!(
			"host key verification failed for {host}:{port}: the server presented {}, which is not the key recorded \
			 on line {} of {}. Either the server's key changed, or this connection is being intercepted. \
			 If the change was expected, remove the stale line from that file.",
			fingerprint(key),
			line + 1,
			path.display()
		),
		Err(e) => Err(anyhow!(e)).with_context(|| {
			format!(
				"host key verification failed for {host}:{port}: the key could not be checked against {}",
				path.display()
			)
		}),
	}
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
/// OpenSSH's own spelling (`SHA256:` plus unpadded base64), so a user can
/// compare it directly against `ssh-keygen -lf` output without re-encoding
/// anything. The ssh2 implementation spelled it in hex only because libssh2
/// handed over raw bytes.
fn fingerprint(key: &ssh_key::PublicKey) -> String {
	key.fingerprint(HashAlg::Sha256).to_string()
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
		assert_eq!(remote_path(&url), "/data/tiles.versatiles");
	}

	#[test]
	fn test_remote_path_root() {
		let url = Url::parse("sftp://host/").unwrap();
		assert_eq!(remote_path(&url), "/");
	}

	#[test]
	fn test_remote_path_nested() {
		let url = Url::parse("sftp://host/a/b/c/d/file.tar").unwrap();
		assert_eq!(remote_path(&url), "/a/b/c/d/file.tar");
	}

	#[test]
	fn test_remote_path_with_credentials() {
		let url = Url::parse("sftp://user:pass@host/data/file.versatiles").unwrap();
		assert_eq!(remote_path(&url), "/data/file.versatiles");
	}

	#[test]
	fn test_remote_path_with_port() {
		let url = Url::parse("sftp://host:2222/data/file.versatiles").unwrap();
		assert_eq!(remote_path(&url), "/data/file.versatiles");
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

	#[tokio::test(flavor = "current_thread")]
	async fn test_open_session_missing_host() {
		// A URL with no host should fail
		let url = Url::parse("sftp:///path/file").unwrap();
		let result = open_session(&url, None).await;
		let err = result.err().expect("expected error for missing host");
		assert!(err.to_string().contains("no host"));
	}

	#[tokio::test(flavor = "current_thread")]
	async fn test_open_session_unreachable_host() {
		// Connection to a non-routable IP should fail with a TCP error
		let url = Url::parse("sftp://192.0.2.1:22222/path").unwrap();
		assert!(open_session(&url, None).await.is_err());
	}

	#[tokio::test(flavor = "current_thread")]
	async fn test_open_session_unresolvable_host() {
		// A hostname that DNS cannot resolve exercises the resolve-error branch
		// in `connect_tcp`.
		let url = Url::parse("sftp://this-host-must-not-exist.invalid:22/path").unwrap();
		let Err(err) = open_session(&url, None).await else {
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
		assert_eq!(remote_path(&url), expected);
	}

	/// Remote paths are `/`-separated whatever the host platform does.
	#[rstest::rstest]
	#[case("/base", "a.bin", "/base/a.bin")]
	#[case("/base/", "a.bin", "/base/a.bin")]
	#[case("/base", "/a.bin", "/base/a.bin")]
	#[case("/base", "a/b/c.bin", "/base/a/b/c.bin")]
	#[case("/", "a.bin", "/a.bin")]
	#[case("", "a.bin", "/a.bin")]
	#[case("/base", "", "/base")]
	fn test_remote_join(#[case] base: &str, #[case] relative: &str, #[case] expected: &str) {
		assert_eq!(remote_join(base, relative), expected);
	}

	#[rstest::rstest]
	#[case("/a/b/c", Some("/a/b"))]
	#[case("/a/b/c/", Some("/a/b"))]
	#[case("/a", None)]
	#[case("/", None)]
	fn test_remote_parent(#[case] path: &str, #[case] expected: Option<&str>) {
		assert_eq!(remote_parent(path), expected);
	}

	/// Sets an environment variable for the duration of a test and restores the
	/// previous value on drop.
	///
	/// `set_var` is unsafe in edition 2024 because another thread may be reading
	/// the environment concurrently. Every test using this guard is marked
	/// `#[serial_test::serial]`, so no other test is running.
	struct EnvGuard(&'static str, Option<std::ffi::OsString>);

	impl EnvGuard {
		fn set(key: &'static str, value: &str) -> Self {
			let previous = std::env::var_os(key);
			unsafe { std::env::set_var(key, value) };
			Self(key, previous)
		}

		fn unset(key: &'static str) -> Self {
			let previous = std::env::var_os(key);
			unsafe { std::env::remove_var(key) };
			Self(key, previous)
		}
	}

	impl Drop for EnvGuard {
		fn drop(&mut self) {
			match self.1.take() {
				Some(value) => unsafe { std::env::set_var(self.0, value) },
				None => unsafe { std::env::remove_var(self.0) },
			}
		}
	}

	fn known_hosts_env(value: &str) -> EnvGuard {
		EnvGuard::set("VERSATILES_SFTP_KNOWN_HOSTS", value)
	}

	/// `off`, `none` and an empty value all mean "do not verify"; anything else
	/// is a path.
	#[test]
	#[serial_test::serial]
	fn known_hosts_path_from_env() {
		{
			let _env = known_hosts_env("off");
			assert_eq!(known_hosts_path().unwrap(), None);
		}
		{
			let _env = known_hosts_env("NONE");
			assert_eq!(known_hosts_path().unwrap(), None);
		}
		{
			let _env = known_hosts_env("  ");
			assert_eq!(known_hosts_path().unwrap(), None);
		}
		{
			let _env = known_hosts_env("/tmp/some/known_hosts");
			assert_eq!(
				known_hosts_path().unwrap(),
				Some(PathBuf::from("/tmp/some/known_hosts"))
			);
		}
	}

	/// The keepalive interval the ssh2 implementation clamped inside its own
	/// thread loop; now it is one value handed to russh, so it can be asserted
	/// on directly.
	#[test]
	#[serial_test::serial]
	fn keepalive_interval_is_configurable_and_floored() {
		{
			let _env = EnvGuard::unset("VERSATILES_SFTP_KEEPALIVE_SECS");
			assert_eq!(keepalive_interval(), Duration::from_secs(15));
		}
		{
			let _env = EnvGuard::set("VERSATILES_SFTP_KEEPALIVE_SECS", "60");
			assert_eq!(keepalive_interval(), Duration::from_secs(60));
		}
		{
			// A zero interval would ask russh to ping on every poll.
			let _env = EnvGuard::set("VERSATILES_SFTP_KEEPALIVE_SECS", "0");
			assert_eq!(keepalive_interval(), Duration::from_secs(1));
		}
	}

	/// `0` keeps its historical meaning of "no timeout"; anything else is a
	/// millisecond budget.
	#[test]
	fn operation_timeout_reads_the_url_in_tests() {
		let url = Url::parse("sftp://host/?timeout_ms=1234").unwrap();
		assert_eq!(operation_timeout(&url), Some(Duration::from_millis(1234)));

		let url = Url::parse("sftp://host/?timeout_ms=0").unwrap();
		assert_eq!(operation_timeout(&url), None);

		let url = Url::parse("sftp://host/").unwrap();
		assert_eq!(operation_timeout(&url), Some(Duration::from_millis(500)));
	}

	/// Points `HOME` at an empty directory and hides any SSH agent for the
	/// duration of a test, so key discovery finds nothing regardless of what the
	/// developer's machine happens to have.
	struct IsolatedHome {
		_dir: tempfile::TempDir,
		_home: EnvGuard,
		_agent: EnvGuard,
	}

	impl IsolatedHome {
		fn set() -> Self {
			let dir = tempfile::tempdir().unwrap();
			let home = EnvGuard::set(
				if cfg!(unix) { "HOME" } else { "USERPROFILE" },
				dir.path().to_str().unwrap(),
			);
			let agent = EnvGuard::unset("SSH_AUTH_SOCK");
			Self {
				_dir: dir,
				_home: home,
				_agent: agent,
			}
		}
	}

	/// With an empty home there are no default keys to offer.
	///
	/// The ssh2 version of this test could only be written by handing an
	/// *unconnected* `Session` to the auth helper, which on a machine that did
	/// have an `~/.ssh/id_rsa` sent libssh2 reading from a socket the session did
	/// not have — a full `cargo test` run was once found parked in `recvfrom` for
	/// over an hour because of it. Key discovery is now a separate function, so
	/// the test needs no session at all.
	#[test]
	#[serial_test::serial]
	fn no_default_keys_are_offered_from_an_empty_home() {
		let _home = IsolatedHome::set();
		assert!(default_key_files().unwrap().is_empty());
	}

	#[test]
	#[serial_test::serial]
	fn the_ssh_config_is_reported_when_there_is_none() {
		let _home = IsolatedHome::set();
		let error = format!("{:#}", config_identity_files("definitely-not-a-real-host").unwrap_err());
		assert!(error.contains(".ssh/config"), "should name where it looked: {error}");
	}

	/// An `~/.ssh/config` that names a key which does not exist yields no
	/// candidates rather than an error, so the caller falls through to the next
	/// method.
	#[test]
	#[serial_test::serial]
	fn a_config_naming_a_missing_key_offers_nothing() {
		let home = IsolatedHome::set();
		let ssh_dir = home._dir.path().join(".ssh");
		std::fs::create_dir_all(&ssh_dir).unwrap();
		std::fs::write(
			ssh_dir.join("config"),
			"Host example\n  IdentityFile ~/.ssh/id_does_not_exist\n",
		)
		.unwrap();

		assert!(config_identity_files("example").unwrap().is_empty());
	}

	#[tokio::test(flavor = "current_thread")]
	#[serial_test::serial]
	async fn no_agent_is_reported_rather_than_hung() {
		let _home = IsolatedHome::set();
		// On unix `SSH_AUTH_SOCK` is unset by the guard, so there is nothing to
		// connect to; on Windows the named pipe is absent unless OpenSSH's agent
		// service happens to be running, which CI images do not start.
		assert!(connect_agent().await.is_err());
	}

	#[cfg(feature = "sftp")]
	mod sftp_server_tests {
		use super::*;
		use crate::io::test_sftp_server::TestSftpServer;

		#[tokio::test(flavor = "current_thread")]
		#[serial_test::serial]
		async fn open_session_password_auth() {
			let server = TestSftpServer::start().await;
			let url = server.url("/");
			let session = open_session(&url, None).await;
			assert!(session.is_ok(), "expected successful auth: {:?}", session.err());
		}

		/// A peer that accepts the connection and then says nothing must not stall
		/// the client for ever.
		///
		/// Under ssh2 this needed `SO_RCVTIMEO` on the socket: libssh2 consulted
		/// its own timeout only once `recv` returned, and on a blocking socket
		/// `recv` waited for a packet that never arrived. A run of this suite was
		/// found parked in `recvfrom` for over an hour. The async transport has no
		/// such hole, but the budget still has to be applied — this test is what
		/// says so.
		#[tokio::test(flavor = "current_thread")]
		async fn a_silent_server_does_not_hang_the_client() {
			use std::{net::TcpListener, time::Instant};

			let listener = TcpListener::bind("127.0.0.1:0").unwrap();
			let port = listener.local_addr().unwrap().port();

			// Accept, then hold the connection open without sending a banner.
			let server = std::thread::spawn(move || {
				let accepted = listener.accept();
				std::thread::sleep(Duration::from_secs(2));
				drop(accepted);
			});

			let url = Url::parse(&format!("sftp://user:pass@127.0.0.1:{port}/?timeout_ms=300")).unwrap();
			let started = Instant::now();
			let result = open_session(&url, None).await;
			let elapsed = started.elapsed();

			assert!(result.is_err(), "a server that never speaks must not authenticate");
			assert!(
				elapsed < Duration::from_millis(1500),
				"open_session waited {elapsed:?}; the handshake timeout should have ended it"
			);

			server.join().ok();
		}

		/// Accept-new: an unknown host connects and its key is written down; the
		/// next connection to the same host matches that record instead of
		/// adding a second one.
		#[tokio::test(flavor = "current_thread")]
		#[serial_test::serial]
		async fn known_hosts_accept_new_records_the_key() {
			let dir = tempfile::tempdir().unwrap();
			let path = dir.path().join("known_hosts");
			let _env = known_hosts_env(path.to_str().unwrap());

			// An existing file whose last line has no terminator: the new entry
			// must not be appended onto it.
			std::fs::write(&path, b"# an existing entry without a trailing newline").unwrap();

			let server = TestSftpServer::start().await;
			let url = server.url("/");
			let port = url.port().unwrap();

			let first = open_session(&url, None).await;
			assert!(first.is_ok(), "first connection failed: {:?}", first.err());

			let recorded = std::fs::read_to_string(&path).unwrap();
			assert!(
				recorded.contains(&format!("[127.0.0.1]:{port}")),
				"host key was not recorded: {recorded:?}"
			);

			let second = open_session(&url, None).await;
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
			let _env = known_hosts_env(path.to_str().unwrap());

			let server_a = TestSftpServer::start().await;
			let url_a = server_a.url("/");
			let port_a = url_a.port().unwrap();
			let recorded = open_session(&url_a, None).await;
			assert!(recorded.is_ok(), "could not record server A: {:?}", recorded.err());

			let server_b = TestSftpServer::start().await;
			let url_b = server_b.url("/");
			let port_b = url_b.port().unwrap();
			assert_ne!(port_a, port_b, "the two test servers reused a port");

			let entry = std::fs::read_to_string(&path)
				.unwrap()
				.replace(&format!("[127.0.0.1]:{port_a}"), &format!("[127.0.0.1]:{port_b}"));
			std::fs::write(&path, entry).unwrap();

			let error = match open_session(&url_b, None).await {
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
			assert!(
				open_session(&url, None).await.is_err(),
				"expected auth failure with wrong password"
			);
		}

		#[tokio::test(flavor = "current_thread")]
		#[serial_test::serial]
		async fn open_session_with_unused_identity_file() {
			let server = TestSftpServer::start().await;
			let url = server.url("/");
			let session = open_session(&url, Some(Path::new("/nonexistent/key"))).await;
			assert!(
				session.is_ok(),
				"password auth should succeed even with a missing identity file"
			);
		}

		#[tokio::test(flavor = "current_thread")]
		#[serial_test::serial]
		async fn an_identity_file_that_is_not_a_key_falls_through_to_the_next_method() {
			// The branch where the file exists but cannot be parsed as a key, as
			// opposed to the already-covered case of it not existing at all.
			let dir = tempfile::tempdir().unwrap();
			let not_a_key = dir.path().join("id_rubbish");
			std::fs::write(&not_a_key, b"this is not a private key\n").unwrap();

			let server = TestSftpServer::start().await;
			let url = server.url("/");
			let session = open_session(&url, Some(&not_a_key)).await;

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
			let _home = IsolatedHome::set();
			let server = TestSftpServer::start().await;
			let mut url = server.url("/");
			url.set_password(None).unwrap();
			url.set_username("nobody").unwrap();
			let target = display_name(&url);

			let result = open_session(&url, None).await;
			let error = format!("{:#}", result.err().expect("no key can authenticate here"));
			assert!(error.contains(&target), "should name the target: {error}");
		}

		/// The SFTP subsystem opens on an authenticated session.
		#[tokio::test(flavor = "current_thread")]
		#[serial_test::serial]
		async fn open_sftp_starts_a_subsystem() {
			let server = TestSftpServer::start().await;
			let url = server.url("/");
			let handle = open_session(&url, None)
				.await
				.expect("test server accepts the password");
			assert!(open_sftp(&handle).await.is_ok());
		}
	}
}
