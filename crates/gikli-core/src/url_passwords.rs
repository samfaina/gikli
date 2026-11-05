//! Passwords in URLs. A clone URL can carry one, as in `https://user:secret@host/repo.git`, and git
//! can repeat it in what it writes. gikli stores no passwords, so whatever it keeps or shows has
//! them hidden. Only git receives the URL as the user gave it.

/// What shows in place of a password.
const HIDDEN: &str = "***";

/// `text` with the password of each URL in it hidden, and everything else as it was.
///
/// The part of a URL that can hold a password ends at the first `/`, `?` or `#`, as git reads it,
/// and at the end of the line. The password runs from the first `:` there to the last `@`, as a
/// password can hold an `@` when it is not encoded. Anything else that reads the same way loses the
/// same part, which is the safe way to get it wrong.
pub(crate) fn hide_url_passwords(text: &str) -> String {
    let mut hidden = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(scheme_end) = rest.find("://") {
        let (before, after) = rest.split_at(scheme_end + "://".len());
        hidden.push_str(before);
        rest = after;
        let end = after
            .find(['/', '?', '#', '\r', '\n'])
            .unwrap_or(after.len());
        let authority = &after[..end];
        if let Some(at) = authority.rfind('@')
            && let Some(colon) = authority[..at].find(':')
        {
            hidden.push_str(&authority[..=colon]);
            hidden.push_str(HIDDEN);
            rest = &after[at..];
        }
    }
    hidden.push_str(rest);
    hidden
}
