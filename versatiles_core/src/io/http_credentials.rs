//! Passwords for HTTP URLs that name a user but give no password.
//!
//! A URL can carry its own credentials, `https://user:password@host/…`, and
//! that keeps working. But a URL is something people store: a pipeline holds it
//! as `from_container filename="…"`, and the password ends up in the `.vpl`
//! file, in a project, in a list of recent sources, and in every copy of any of
//! them. So a URL may name only the user, `https://user@host/…`, and the
//! password is looked up here, in this order:
//!
//! 1. the password written into the URL, if there is one;
//! 2. the store an embedding application fills in with [`set_http_password`],
//!    for instance from the system keychain;
//! 3. `~/.netrc`, but only when enabled with `VERSATILES_HTTP_NETRC=on` or
//!    [`enable_http_netrc`] (the CLI's `--netrc`).
//!
//! If none has one, the request is sent with the user and no password.
//!
//! A credential is matched by the server and the user name, never by the path,
//! so it cannot reach another server, and a URL with no user never picks one
//! up. In the store, the server is the origin, so `http://host` does not get the
//! password registered for `https://host`. A `.netrc` entry names only a host,
//! as curl, git and wget read it.

use std::{
	ffi::OsString,
	path::{Path, PathBuf},
	sync::{
		LazyLock,
		atomic::{AtomicBool, Ordering},
	},
};

use anyhow::{Result, bail};
use dashmap::DashMap;
use reqwest::Url;

/// Passwords by origin (`https://host:port`, default port omitted) and user.
static STORE: LazyLock<DashMap<(String, String), String>> = LazyLock::new(DashMap::new);

/// Set by [`enable_http_netrc`].
static NETRC_ENABLED: AtomicBool = AtomicBool::new(false);

/// The origin a credential belongs to: scheme, host and port.
fn origin_of(url: &Url) -> Result<String> {
	match url.scheme() {
		"http" | "https" => Ok(url.origin().ascii_serialization()),
		other => bail!("an HTTP password belongs to an http or https URL, not to a '{other}' one"),
	}
}

/// Registers the password for `user` on the server `origin` names.
///
/// Only the scheme, host and port of `origin` count: a password registered for
/// `https://dav.example.org/remote.php/` is sent to every URL on
/// `https://dav.example.org` that names `user` and gives no password of its
/// own, and to no other server. A later call for the same server and user
/// replaces the password.
///
/// The store belongs to the process, so this is for an application that keeps
/// passwords somewhere safe, such as the system keychain, and hands them over
/// at start-up. A pipeline then only has to say `https://user@host/…`, which is
/// safe to store and to share.
///
/// # Errors
///
/// If `origin` is not an `http` or `https` URL.
///
/// # Examples
///
/// ```
/// use reqwest::Url;
/// use versatiles_core::io::{forget_http_password, set_http_password};
///
/// let server = Url::parse("https://dav.example.org").unwrap();
/// set_http_password(&server, "alice", "secret").unwrap();
/// forget_http_password(&server, "alice").unwrap();
/// ```
pub fn set_http_password(origin: &Url, user: &str, password: &str) -> Result<()> {
	STORE.insert((origin_of(origin)?, user.to_string()), password.to_string());
	Ok(())
}

/// Removes the password [`set_http_password`] registered for `user` on the
/// server `origin` names, if there is one.
///
/// # Errors
///
/// If `origin` is not an `http` or `https` URL.
pub fn forget_http_password(origin: &Url, user: &str) -> Result<()> {
	STORE.remove(&(origin_of(origin)?, user.to_string()));
	Ok(())
}

/// Lets a URL with a user but no password take it from `~/.netrc`.
///
/// The same as setting `VERSATILES_HTTP_NETRC=on`, and what the CLI's
/// `--netrc` does. Off by default, for two reasons: a pipeline is a file people
/// share, and opening someone else's that names `https://you@your-server/…`
/// should not sign in to your server with your password without your deciding
/// to; and a library should not read a credentials file the program embedding
/// it never asked for.
///
/// The file is the one `NETRC` names, as for curl, or else `~/.netrc` (and on
/// Windows `%USERPROFILE%\_netrc` if there is no `.netrc`). An entry counts
/// only when its `machine` is the URL's host and its `login` is the URL's user;
/// a `default` entry is never used, since it would hand the password to any
/// server a URL names.
pub fn enable_http_netrc() {
	NETRC_ENABLED.store(true, Ordering::Relaxed);
}

/// Whether `~/.netrc` may be read, from [`enable_http_netrc`] and the
/// environment.
pub(crate) fn netrc_enabled() -> bool {
	NETRC_ENABLED.load(Ordering::Relaxed) || netrc_enabled_by(std::env::var("VERSATILES_HTTP_NETRC").ok().as_deref())
}

/// Whether the value of `VERSATILES_HTTP_NETRC` enables `~/.netrc`.
fn netrc_enabled_by(value: Option<&str>) -> bool {
	match value.map(str::trim) {
		Some("on") => true,
		None | Some("" | "off") => false,
		Some(other) => {
			log::warn!("ignoring VERSATILES_HTTP_NETRC={other:?}: expected 'on' or 'off'");
			false
		}
	}
}

/// The password the store has for `user` on the server of `url`.
pub(crate) fn stored_password(url: &Url, user: &str) -> Option<String> {
	let origin = origin_of(url).ok()?;
	STORE
		.get(&(origin, user.to_string()))
		.map(|entry| entry.value().clone())
}

/// The `.netrc` file to read, or `None` if reading it is not enabled.
pub(crate) fn netrc_file() -> Option<PathBuf> {
	netrc_file_from(
		netrc_enabled(),
		std::env::var_os("NETRC"),
		crate::io::home_dir(),
		cfg!(windows),
	)
}

fn netrc_file_from(enabled: bool, netrc: Option<OsString>, home: Option<PathBuf>, windows: bool) -> Option<PathBuf> {
	if !enabled {
		return None;
	}
	if let Some(path) = netrc.filter(|path| !path.is_empty()) {
		return Some(PathBuf::from(path));
	}
	let home = home?;
	let dot = home.join(".netrc");
	if windows && !dot.exists() {
		let underscore = home.join("_netrc");
		if underscore.exists() {
			return Some(underscore);
		}
	}
	Some(dot)
}

/// The password `file` has for `user` on `host`, reading it now.
///
/// A missing file is no password; one that cannot be read is too, with a
/// warning, since the request may still succeed without it.
pub(crate) fn netrc_password(file: &Path, host: &str, user: &str) -> Option<String> {
	match std::fs::read_to_string(file) {
		Ok(text) => netrc_lookup(&text, host, user),
		Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
			log::debug!(
				"{} does not exist, so it has no password for {user} on {host}",
				file.display()
			);
			None
		}
		Err(error) => {
			log::warn!("could not read {}: {error}", file.display());
			None
		}
	}
}

/// The password of the first entry in `.netrc` text whose `machine` is `host`
/// and whose `login` is `user`.
///
/// Reads the format curl, git and wget read: `machine`, `login`, `password`
/// and `account` followed by a value, `default` starting an entry for any host,
/// and `macdef` a macro that runs to the next blank line. Lines starting with
/// `#` are comments, and a value may be written in double quotes, with `\"` and
/// `\\` inside. Host names compare without regard to case.
fn netrc_lookup(text: &str, host: &str, user: &str) -> Option<String> {
	/// The entry being read.
	#[derive(Default)]
	struct Entry {
		matches_host: bool,
		login: Option<String>,
		password: Option<String>,
	}

	impl Entry {
		fn password_for(self, user: &str) -> Option<String> {
			if self.matches_host && self.login.as_deref() == Some(user) {
				self.password
			} else {
				None
			}
		}
	}

	let host = host.trim_start_matches('[').trim_end_matches(']');
	let mut entry: Option<Entry> = None;
	let mut in_macro = false;
	let mut pending_key: Option<String> = None;

	for line in text.lines() {
		if in_macro {
			in_macro = !line.trim().is_empty();
			continue;
		}
		if line.trim_start().starts_with('#') {
			continue;
		}

		for token in netrc_tokens(line) {
			if let Some(key) = pending_key.take() {
				if let Some(entry) = entry.as_mut() {
					match key.as_str() {
						"machine" => entry.matches_host = token.eq_ignore_ascii_case(host),
						"login" => entry.login = Some(token),
						"password" => entry.password = Some(token),
						_ => {}
					}
				}
				continue;
			}

			match token.as_str() {
				"machine" | "default" | "macdef" => {
					if let Some(password) = entry.take().and_then(|e| e.password_for(user)) {
						return Some(password);
					}
					match token.as_str() {
						"machine" => {
							entry = Some(Entry::default());
							pending_key = Some(token);
						}
						// Never matches: `matches_host` stays false.
						"default" => entry = Some(Entry::default()),
						_ => {
							// The macro's name is the rest of this line; its body
							// runs to the next blank line.
							in_macro = true;
							break;
						}
					}
				}
				"login" | "password" | "account" => pending_key = Some(token),
				// Unknown words are skipped, as curl skips them.
				_ => {}
			}
		}
	}

	entry.and_then(|e| e.password_for(user))
}

/// The whitespace-separated words of a `.netrc` line, with double-quoted ones
/// unquoted.
fn netrc_tokens(line: &str) -> Vec<String> {
	let mut tokens = Vec::new();
	let mut chars = line.chars().peekable();
	while let Some(&c) = chars.peek() {
		if c.is_whitespace() {
			chars.next();
			continue;
		}
		let mut token = String::new();
		if c == '"' {
			chars.next();
			while let Some(c) = chars.next() {
				match c {
					'"' => break,
					'\\' => {
						if let Some(escaped) = chars.next() {
							token.push(escaped);
						}
					}
					c => token.push(c),
				}
			}
		} else {
			while let Some(&c) = chars.peek() {
				if c.is_whitespace() {
					break;
				}
				token.push(c);
				chars.next();
			}
		}
		tokens.push(token);
	}
	tokens
}

#[cfg(test)]
mod tests {
	use super::*;

	fn url(text: &str) -> Url {
		Url::parse(text).unwrap()
	}

	/// The store is shared by every test in the process, so each test here uses
	/// hosts no other test uses.
	#[test]
	fn a_stored_password_reaches_only_its_server_and_user() {
		set_http_password(&url("https://store-a.invalid/some/path/"), "alice", "secret").unwrap();

		assert_eq!(
			stored_password(&url("https://store-a.invalid/other/file.pmtiles"), "alice").as_deref(),
			Some("secret")
		);
		// The default port is the same origin.
		assert_eq!(
			stored_password(&url("https://store-a.invalid:443/x"), "alice").as_deref(),
			Some("secret")
		);

		for (other, user) in [
			("https://store-a.invalid/x", "bob"),
			("https://store-a.invalid:8443/x", "alice"),
			("http://store-a.invalid/x", "alice"),
			("https://store-b.invalid/x", "alice"),
			("https://sub.store-a.invalid/x", "alice"),
		] {
			assert_eq!(stored_password(&url(other), user), None, "{user} on {other}");
		}
	}

	#[test]
	fn a_forgotten_password_is_gone() {
		let server = url("https://store-forget.invalid");
		set_http_password(&server, "alice", "first").unwrap();
		set_http_password(&server, "alice", "second").unwrap();
		assert_eq!(stored_password(&server, "alice").as_deref(), Some("second"));

		forget_http_password(&server, "alice").unwrap();
		assert_eq!(stored_password(&server, "alice"), None);
		// Forgetting what is not there is fine.
		forget_http_password(&server, "alice").unwrap();
	}

	#[test]
	fn only_http_urls_take_a_password() {
		let error = set_http_password(&url("sftp://store-sftp.invalid/"), "alice", "x").unwrap_err();
		assert!(error.to_string().contains("'sftp'"), "got: {error}");
	}

	#[test]
	fn netrc_is_enabled_only_by_on() {
		assert!(netrc_enabled_by(Some("on")));
		assert!(netrc_enabled_by(Some(" on ")));
		for value in [None, Some(""), Some("off"), Some("1"), Some("yes"), Some("ON")] {
			assert!(!netrc_enabled_by(value), "{value:?}");
		}
	}

	#[test]
	fn the_netrc_file_is_none_unless_enabled() {
		let home = Some(PathBuf::from("/home/alice"));
		assert_eq!(
			netrc_file_from(false, Some("/x/netrc".into()), home.clone(), false),
			None
		);
		assert_eq!(netrc_file_from(false, None, home, false), None);
	}

	#[test]
	fn the_netrc_file_is_named_by_netrc_or_found_at_home() {
		let home = Some(PathBuf::from("/home/alice"));
		assert_eq!(
			netrc_file_from(true, Some("/x/netrc".into()), home.clone(), false),
			Some(PathBuf::from("/x/netrc"))
		);
		assert_eq!(
			netrc_file_from(true, Some(OsString::new()), home.clone(), false),
			Some(PathBuf::from("/home/alice/.netrc"))
		);
		assert_eq!(
			netrc_file_from(true, None, home, false),
			Some(PathBuf::from("/home/alice/.netrc"))
		);
		assert_eq!(netrc_file_from(true, None, None, false), None);
	}

	#[test]
	fn on_windows_an_underscore_netrc_is_used_when_there_is_no_dot_netrc() {
		let dir = tempfile::tempdir().unwrap();
		let home = Some(dir.path().to_path_buf());
		assert_eq!(
			netrc_file_from(true, None, home.clone(), true),
			Some(dir.path().join(".netrc"))
		);

		std::fs::write(dir.path().join("_netrc"), "").unwrap();
		assert_eq!(
			netrc_file_from(true, None, home.clone(), true),
			Some(dir.path().join("_netrc"))
		);
		assert_eq!(
			netrc_file_from(true, None, home.clone(), false),
			Some(dir.path().join(".netrc"))
		);

		std::fs::write(dir.path().join(".netrc"), "").unwrap();
		assert_eq!(netrc_file_from(true, None, home, true), Some(dir.path().join(".netrc")));
	}

	#[test]
	fn a_netrc_entry_counts_only_for_its_host_and_login() {
		let text = "\
machine dav.example.org login alice password one
machine dav.example.org
  login bob
  password two
machine other.example.org login alice password three
";
		assert_eq!(netrc_lookup(text, "dav.example.org", "alice").as_deref(), Some("one"));
		assert_eq!(netrc_lookup(text, "DAV.Example.org", "bob").as_deref(), Some("two"));
		assert_eq!(
			netrc_lookup(text, "other.example.org", "alice").as_deref(),
			Some("three")
		);
		assert_eq!(netrc_lookup(text, "dav.example.org", "carol"), None);
		assert_eq!(netrc_lookup(text, "nowhere.example.org", "alice"), None);
	}

	#[test]
	fn a_default_entry_is_never_used() {
		let text = "default login alice password everywhere\n";
		assert_eq!(netrc_lookup(text, "dav.example.org", "alice"), None);

		let text = "default login alice password everywhere\nmachine dav.example.org login alice password here\n";
		assert_eq!(netrc_lookup(text, "dav.example.org", "alice").as_deref(), Some("here"));
	}

	#[test]
	fn the_first_matching_entry_wins() {
		let text = "machine h login alice password first\nmachine h login alice password second\n";
		assert_eq!(netrc_lookup(text, "h", "alice").as_deref(), Some("first"));
	}

	#[test]
	fn an_entry_without_a_login_matches_nobody() {
		assert_eq!(netrc_lookup("machine h password p\n", "h", "alice"), None);
		assert_eq!(netrc_lookup("machine h password p\n", "h", ""), None);
	}

	#[test]
	fn comments_macros_quotes_and_accounts_are_read() {
		let text = r#"
# a comment: machine h login alice password wrong
macdef init
machine h login alice password inside-a-macro

machine h account acme login alice password "with space \"and quote\" \\"
"#;
		assert_eq!(
			netrc_lookup(text, "h", "alice").as_deref(),
			Some(r#"with space "and quote" \"#)
		);
	}

	#[test]
	fn an_ipv6_host_matches_without_brackets() {
		assert_eq!(
			netrc_lookup("machine ::1 login alice password p\n", "[::1]", "alice").as_deref(),
			Some("p")
		);
	}

	#[test]
	fn a_missing_netrc_file_has_no_password() {
		let dir = tempfile::tempdir().unwrap();
		assert_eq!(netrc_password(&dir.path().join("missing"), "h", "alice"), None);

		let file = dir.path().join("netrc");
		std::fs::write(&file, "machine h login alice password p\n").unwrap();
		assert_eq!(netrc_password(&file, "h", "alice").as_deref(), Some("p"));
	}
}
