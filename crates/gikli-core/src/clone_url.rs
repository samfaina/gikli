//! The HTTPS and SSH forms of the URL of a repository, for the user to choose between. The form a
//! URL is already in stays as the user typed it, password included: it is what they asked git to
//! use. The other form takes only the host and the path, so it never carries a password.

use serde::Serialize;

use crate::url_passwords::hide_url_passwords;

/// The two forms of a URL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloneUrlForms {
    /// A URL on plain HTTP stays as it is here, as its server may have no HTTPS to move to.
    pub https: String,
    pub ssh: String,
}

/// The forms of `url`, when it is a URL on HTTP or HTTPS, or either way of writing a URL on SSH.
/// Any other, such as a folder on this machine, has none.
pub(crate) fn forms_of(url: &str) -> Option<CloneUrlForms> {
    // Read with its password hidden, so no part of it can be taken for the host or the path.
    let hidden = hide_url_passwords(url);
    if let Some(rest) = ["https://", "http://"]
        .iter()
        .find_map(|scheme| hidden.strip_prefix(scheme))
    {
        let (host, path) = host_and_path(rest)?;
        return Some(CloneUrlForms {
            https: url.to_owned(),
            ssh: format!("git@{host}:{path}"),
        });
    }
    let (host, path) = match hidden.strip_prefix("ssh://") {
        Some(rest) => host_and_path(rest)?,
        None => scp_host_and_path(&hidden)?,
    };
    Some(CloneUrlForms {
        https: format!("https://{host}/{}", path.trim_start_matches('/')),
        ssh: url.to_owned(),
    })
}

/// The host and the path, without the `/` at its start, in what follows the `://` of a URL. A port
/// belongs to the protocol it was typed for, so it goes.
fn host_and_path(rest: &str) -> Option<(&str, &str)> {
    let (authority, path) = rest.split_once('/')?;
    let host_and_port = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    // An IPv6 address, in brackets, holds colons of its own.
    let host = match host_and_port.rsplit_once(':') {
        Some((host, port)) if !port.ends_with(']') => port
            .bytes()
            .all(|byte| byte.is_ascii_digit())
            .then_some(host)?,
        _ => host_and_port,
    };
    usable(host, path).then_some((host, path))
}

/// The host and the path of a URL written as `[user@]host:path`, as git reads it: a `/` before the
/// first `:` makes it a folder on this machine, and so does a single letter on Windows, as a drive.
fn scp_host_and_path(url: &str) -> Option<(&str, &str)> {
    if url.contains("://") {
        return None;
    }
    let after_bracket = url.find(']').map_or(0, |bracket| bracket + 1);
    let colon = after_bracket + url[after_bracket..].find(':')?;
    let (user_and_host, path) = (&url[..colon], &url[colon + 1..]);
    if user_and_host.contains(['/', '\\']) {
        return None;
    }
    let host = match user_and_host.rsplit_once('@') {
        Some(("", _)) => return None,
        Some((_, host)) => host,
        None if user_and_host.len() == 1 => return None,
        None => user_and_host,
    };
    usable(host, path).then_some((host, path))
}

/// Whether `host` and `path` can make up the URL of a repository in another form. A host that starts
/// with a `-` reads as an option of ssh, which git refuses to pass on, and what follows a `?` or a
/// `#` only means something in the form it was typed in.
fn usable(host: &str, path: &str) -> bool {
    !host.is_empty()
        && !host.starts_with('-')
        && !host.contains(char::is_whitespace)
        && !path.trim_start_matches('/').is_empty()
        && !path.contains(['?', '#'])
        && !path.contains(char::is_whitespace)
}
