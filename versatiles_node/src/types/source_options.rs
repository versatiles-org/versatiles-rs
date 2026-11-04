use napi_derive::napi;

/// Options for opening a tile source
///
/// Everything here is optional; an omitted field keeps the default behaviour.
#[napi(object)]
#[derive(Clone, Default)]
pub struct SourceOptions {
	/// SSH identity (private key) file used for `sftp://` sources
	///
	/// A path to a private key file on disk — the same thing the CLI's
	/// `--ssh-identity` names. It is read when the source is an `sftp://` URL,
	/// including one reached through a pipeline, and ignored otherwise.
	///
	/// Without it, the `VERSATILES_SSH_IDENTITY` environment variable is used;
	/// without that, a password in the URL, the SSH agent and the usual `~/.ssh`
	/// defaults still apply. Set this to give one source a different key than the
	/// rest of the process.
	///
	/// **Example:** `'/home/deploy/.ssh/id_ed25519'`
	///
	/// **Default:** `VERSATILES_SSH_IDENTITY`, if set
	pub ssh_identity: Option<String>,

	/// Directory for the disk cache
	///
	/// Puts the cache of this source on disk instead of in memory — the same
	/// thing the CLI's `--cache-dir` does. It matters for a source whose tiles
	/// are cached on the way, such as a large pipeline or a conversion from it.
	/// The directory is created if it does not exist.
	///
	/// Without it, the `VERSATILES_CACHE_DIR` environment variable is used;
	/// without that, the cache stays in memory.
	///
	/// **Example:** `'/var/tmp/versatiles'`
	///
	/// **Default:** `VERSATILES_CACHE_DIR`, if set, otherwise in memory
	pub cache_dir: Option<String>,
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn source_options_default_is_empty() {
		assert!(SourceOptions::default().ssh_identity.is_none());
	}

	#[test]
	fn source_options_clone() {
		let options = SourceOptions {
			ssh_identity: Some("/keys/deploy".to_string()),
			cache_dir: Some("/var/tmp/versatiles".to_string()),
		};
		assert_eq!(options.clone().ssh_identity, options.ssh_identity);
	}
}
