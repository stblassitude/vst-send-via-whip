//! Just enough URL handling to resolve the WHIP `Location` header.

/// Resolves `reference` against the absolute URL `base` (RFC 3986, without
/// dot-segment removal beyond what WHIP servers produce in practice).
pub(crate) fn resolve(base: &str, reference: &str) -> Option<String> {
    let reference = reference.trim();
    if reference.starts_with("http://") || reference.starts_with("https://") {
        return Some(reference.to_string());
    }
    let (scheme, rest) = base.split_once("://")?;
    let authority_end = rest.find('/').unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    if authority.is_empty() {
        return None;
    }
    if let Some(r) = reference.strip_prefix("//") {
        return Some(format!("{scheme}://{r}"));
    }
    if reference.starts_with('/') {
        return Some(format!("{scheme}://{authority}{reference}"));
    }
    let path = rest[authority_end..].split(['?', '#']).next().unwrap_or("");
    let dir = match path.rfind('/') {
        Some(i) => &path[..=i],
        None => "/",
    };
    Some(format!("{scheme}://{authority}{dir}{reference}"))
}

#[cfg(test)]
mod tests {
    use super::resolve;

    const BASE: &str = "https://galene.example.org:8443/group/a/.whip";

    #[test]
    fn absolute_path() {
        assert_eq!(
            resolve(BASE, "/group/a/.whip/abc").unwrap(),
            "https://galene.example.org:8443/group/a/.whip/abc"
        );
    }

    #[test]
    fn absolute_url() {
        assert_eq!(
            resolve(BASE, "https://other.example/x").unwrap(),
            "https://other.example/x"
        );
    }

    #[test]
    fn relative_path() {
        assert_eq!(
            resolve(BASE, "session/1").unwrap(),
            "https://galene.example.org:8443/group/a/session/1"
        );
        assert_eq!(resolve("http://h", "s").unwrap(), "http://h/s");
    }

    #[test]
    fn rejects_garbage_base() {
        assert!(resolve("not a url", "/x").is_none());
    }
}
