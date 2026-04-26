use std::time::Duration;

/// Normalize a user-entered CouchDB URL:
///   - default scheme to `http://`
///   - add the CouchDB default port (5984) if no port was specified
///   - strip path / query / fragment
///
/// Returns the canonical URL (no trailing slash).
pub fn normalize_couchdb_url(input: &str) -> Result<String, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("CouchDB URL is required".into());
    }
    let with_scheme = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("http://{trimmed}")
    };
    let mut parsed = url::Url::parse(&with_scheme)
        .map_err(|e| format!("invalid URL: {e}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(format!("URL scheme must be http or https, not {}", parsed.scheme()));
    }
    if parsed.host().is_none() {
        return Err("URL must include a host".into());
    }
    if parsed.port().is_none() {
        parsed
            .set_port(Some(5984))
            .map_err(|_| "could not set default port on URL".to_string())?;
    }
    parsed.set_path("");
    parsed.set_query(None);
    parsed.set_fragment(None);
    Ok(parsed.to_string().trim_end_matches('/').to_string())
}

/// Probe CouchDB to confirm it is reachable and that the credentials are valid.
/// On success returns the normalized URL — the caller should persist that
/// (not the raw user input) so `server.toml` always carries an explicit port.
pub async fn test(
    couchdb_url: &str,
    user: &str,
    password: &str,
) -> Result<String, String> {
    let normalized = normalize_couchdb_url(couchdb_url)?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| format!("HTTP client error: {e}"))?;
    // `_all_dbs` requires admin auth, so a 200 here proves both reachability
    // and that the credentials are accepted.
    let probe = format!("{normalized}/_all_dbs");
    let resp = client
        .get(&probe)
        .basic_auth(user, Some(password))
        .send()
        .await
        .map_err(|e| format!("could not reach {normalized}: {e}"))?;
    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err("CouchDB rejected the username/password.".into());
    }
    if !status.is_success() {
        return Err(format!("CouchDB returned HTTP {status}"));
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_default_port() {
        assert_eq!(
            normalize_couchdb_url("http://192.168.64.3").unwrap(),
            "http://192.168.64.3:5984"
        );
    }

    #[test]
    fn preserves_explicit_port() {
        assert_eq!(
            normalize_couchdb_url("http://localhost:6984").unwrap(),
            "http://localhost:6984"
        );
    }

    #[test]
    fn defaults_scheme() {
        assert_eq!(
            normalize_couchdb_url("localhost").unwrap(),
            "http://localhost:5984"
        );
    }

    #[test]
    fn strips_trailing_path() {
        assert_eq!(
            normalize_couchdb_url("http://localhost:5984/_utils/").unwrap(),
            "http://localhost:5984"
        );
    }

    #[test]
    fn rejects_empty() {
        assert!(normalize_couchdb_url("   ").is_err());
    }
}
