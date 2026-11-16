//! Passwords for `http://` and `https://` sources that name only a user.
//!
//! The same lookup the CLI uses: a password written into the URL wins, then
//! one registered here, then `~/.netrc` if [`enable_http_netrc`] was called or
//! `VERSATILES_HTTP_NETRC=on` is set. All of it belongs to the process, so it
//! counts for every `TileSource`, `convert` and `TileServer` from then on.

use napi::{Result, bindgen_prelude::*};
use napi_derive::napi;
use versatiles_core::io;

use crate::macros::NapiResultExt;

/// The origin `origin` names, as the store keys it.
fn parse_origin(origin: &str) -> Result<reqwest::Url> {
	reqwest::Url::parse(origin).map_err(|error| Error::from_reason(format!("'{origin}' is not a URL: {error}")))
}

/// Registers the password for `user` on the server `origin` names.
///
/// Only the scheme, host and port of `origin` count: a password registered for
/// `https://dav.example.org/remote.php/` is sent to every URL on
/// `https://dav.example.org` that names `user` and has no password of its own,
/// and to no other server, `http://dav.example.org` included. A later call for
/// the same server and user replaces the password.
///
/// So a source can be named as `https://user@host/…`, which is safe to store
/// and to share, while the password comes from wherever the application keeps
/// it.
///
/// ```javascript
/// import { setHttpPassword, TileSource } from '@versatiles/versatiles-rs';
///
/// setHttpPassword('https://dav.example.org', 'alice', process.env.DAV_PASSWORD);
/// const source = await TileSource.fromPath('https://alice@dav.example.org/tiles.pmtiles');
/// ```
#[napi]
#[allow(
	clippy::needless_pass_by_value,
	reason = "napi passes strings owned; `#[expect]` is never marked fulfilled on a `#[napi]` function"
)]
pub fn set_http_password(origin: String, user: String, password: String) -> Result<()> {
	io::set_http_password(&parse_origin(&origin)?, &user, &password).to_napi()
}

/// Removes the password `setHttpPassword` registered for `user` on the server
/// `origin` names, if there is one.
#[napi]
#[allow(
	clippy::needless_pass_by_value,
	reason = "napi passes strings owned; `#[expect]` is never marked fulfilled on a `#[napi]` function"
)]
pub fn forget_http_password(origin: String, user: String) -> Result<()> {
	io::forget_http_password(&parse_origin(&origin)?, &user).to_napi()
}

/// Lets a source that names a user but no password take it from `~/.netrc`.
///
/// The same as `VERSATILES_HTTP_NETRC=on`, or `--netrc` on the command line,
/// and like them it cannot be turned off again. `NETRC` names another file. An
/// entry counts only when its `machine` is the URL's host and its `login` the
/// URL's user, a `default` entry is never used, and the password is sent only
/// over `https`, or over `http` to this machine.
///
/// Off by default, so that a pipeline someone else wrote cannot sign in with
/// the user's password without the application deciding to.
#[napi]
pub fn enable_http_netrc() {
	io::enable_http_netrc();
}
