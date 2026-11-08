//! Passwords in URLs. A clone URL can carry one, as in `https://user:secret@host/repo.git`, and git
//! can repeat it in what it writes. gikli stores no passwords, so whatever it keeps or shows has
//! them hidden. Only git receives the URL as the user gave it.

/// What shows in place of a password.
const MASK: &str = "***";

/// `text` with the password of each URL in it hidden, and everything else as it was. A `file` URL
/// has no password to hide, so all of it stays.
pub(crate) fn hide_url_passwords(text: &str) -> String {
    let mut shown = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(scheme_end) = rest.find("://") {
        let (before, after) = rest.split_at(scheme_end + "://".len());
        shown.push_str(before);
        rest = after;
        if ends_with_file_scheme(before) {
            continue;
        }
        if let Some((colon, at)) = password_bounds(after) {
            shown.push_str(&after[..=colon]);
            shown.push_str(MASK);
            rest = &after[at..];
        }
    }
    shown.push_str(rest);
    shown
}

/// Whether `text`, which ends at the `://` of a URL, gives that URL the scheme `file`. Only the
/// lowercase word is, as git reads no other as a `file` URL, and a longer scheme that ends in it,
/// such as `myfile` or `git+file`, is not.
fn ends_with_file_scheme(text: &str) -> bool {
    text.strip_suffix("file://").is_some_and(|start| {
        !start.ends_with(|c: char| c.is_alphanumeric() || matches!(c, '+' | '-' | '.' | '_'))
    })
}

/// The `:` before the password and the `@` after it, in what follows the `://` of a URL.
///
/// A password holds whatever the user typed, encoded or not, so its `@` is the last one before the
/// first `/` of the line, and `?` and `#` are part of it. When no `@` comes before that `/`, the
/// password itself may hold the `/`. It then runs to the last `@` of the line, or, when what follows
/// the `:` reads as a port, to the last `@` of the URL, which ends at whitespace or a quote. Other
/// text that reads the same way loses the same part, which is the safe way to get it wrong. A URL
/// with a port and an `@` in its path is such text. A password that starts with zero or more digits
/// and a `/`, then holds whitespace or a quote, stays, as hiding it would also hide ordinary text in
/// git's messages.
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
    let where_the_at_can_be = if reads_as_port {
        let url_end = line.find(|c: char| c.is_whitespace() || c == '\'' || c == '"');
        &line[..url_end.unwrap_or(line.len())]
    } else {
        line
    };
    where_the_at_can_be.rfind('@').map(|at| (colon, at))
}
