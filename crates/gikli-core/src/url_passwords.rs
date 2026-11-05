//! Passwords in URLs. A clone URL can carry one, as in `https://user:secret@host/repo.git`, and git
//! can repeat it in what it writes. gikli stores no passwords, so whatever it keeps or shows has
//! them hidden. Only git receives the URL as the user gave it.

/// What shows in place of a password.
const MASK: &str = "***";

/// `text` with the password of each URL in it hidden, and everything else as it was.
pub(crate) fn hide_url_passwords(text: &str) -> String {
    let mut shown = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(scheme_end) = rest.find("://") {
        let (before, after) = rest.split_at(scheme_end + "://".len());
        shown.push_str(before);
        rest = after;
        if let Some((colon, at)) = password_bounds(after) {
            shown.push_str(&after[..=colon]);
            shown.push_str(MASK);
            rest = &after[at..];
        }
    }
    shown.push_str(rest);
    shown
}

/// The `:` before the password and the `@` after it, in what follows the `://` of a URL.
///
/// A password holds whatever the user typed, encoded or not, so its `@` is the last one before the
/// first `/` of the line, and `?` and `#` are part of it. When no `@` comes before that `/`, the
/// password itself may hold the `/`, unless what follows the `:` reads as a port. It then runs to
/// the last `@` of the line. Other text that reads the same way loses the same part, which is the
/// safe way to get it wrong; a password that starts with digits and a `/` reads as a port and stays.
fn password_bounds(url: &str) -> Option<(usize, usize)> {
    let line = url.split(['\r', '\n']).next().unwrap_or(url);
    let authority = &line[..line.find('/').unwrap_or(line.len())];
    if let Some(at) = authority.rfind('@') {
        return authority[..at].find(':').map(|colon| (colon, at));
    }
    // The colons of an IPv6 address.
    if authority.starts_with('[') {
        return None;
    }
    let colon = authority.find(':')?;
    let reads_as_port = authority[colon + 1..]
        .bytes()
        .all(|byte| byte.is_ascii_digit());
    if reads_as_port {
        return None;
    }
    line.rfind('@').map(|at| (colon, at))
}
