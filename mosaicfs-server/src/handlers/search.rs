use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;

use std::collections::HashSet;

use crate::state::AppState;

#[derive(Deserialize, Default)]
pub struct SearchQuery {
    pub q: Option<String>,
    pub label: Option<String>,
    pub limit: Option<u64>,
    pub offset: Option<u64>,
}

pub async fn search(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SearchQuery>,
) -> impl IntoResponse {
    // If label filter is set, build set of file IDs with that label
    let label_file_ids: Option<HashSet<String>> = if let Some(ref label) = query.label {
        let mut ids = HashSet::new();

        // Check direct assignments
        if let Ok(resp) = state.db.all_docs_by_prefix("label_assignment::", true).await {
            for row in &resp.rows {
                if let Some(doc) = &row.doc {
                    if let Some(labels) = doc.get("labels").and_then(|v| v.as_array()) {
                        if labels.iter().any(|l| l.as_str() == Some(label)) {
                            if let Some(fid) = doc.get("file_id").and_then(|v| v.as_str()) {
                                ids.insert(fid.to_string());
                            }
                        }
                    }
                }
            }
        }

        Some(ids)
    } else {
        None
    };

    let resp = match state.db.all_docs_by_prefix("file::", true).await {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": { "code": "internal", "message": e.to_string() } })),
            );
        }
    };

    let search_term = query.q.as_deref().unwrap_or("");

    let mut items: Vec<serde_json::Value> = resp
        .rows
        .into_iter()
        .filter_map(|row| {
            let doc = row.doc?;
            if doc.get("type")?.as_str()? != "file" {
                return None;
            }
            if doc.get("status").and_then(|v| v.as_str()) != Some("active") {
                return None;
            }

            let name = doc.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let file_id = doc.get("_id").and_then(|v| v.as_str()).unwrap_or("");

            // Match by search term (substring or glob)
            if !search_term.is_empty() && !matches_query(name, search_term) {
                return None;
            }

            // Filter by label
            if let Some(ref ids) = label_file_ids {
                if !ids.contains(file_id) {
                    return None;
                }
            }

            // Build result
            Some(serde_json::json!({
                "id": doc.get("_id"),
                "name": name,
                "source": doc.get("source"),
                "size": doc.get("size"),
                "mtime": doc.get("mtime"),
                "mime_type": doc.get("mime_type"),
            }))
        })
        .collect();

    let total = items.len();
    let offset = query.offset.unwrap_or(0) as usize;
    let limit = query.limit.unwrap_or(100) as usize;
    let items: Vec<_> = items.into_iter().skip(offset).take(limit).collect();

    (StatusCode::OK, Json(serde_json::json!({
        "items": items,
        "total": total,
        "offset": offset,
        "limit": limit,
    })))
}

/// Match a filename against a query string.
/// Supports substring matching and simple glob patterns (* and ?).
fn matches_query(name: &str, query: &str) -> bool {
    if query.contains('*') || query.contains('?') {
        glob_match(name, query)
    } else {
        // Case-insensitive substring match
        name.to_lowercase().contains(&query.to_lowercase())
    }
}

/// Simple glob matching supporting * (any chars) and ? (single char)
fn glob_match(text: &str, pattern: &str) -> bool {
    let text = text.to_lowercase();
    let pattern = pattern.to_lowercase();
    let text = text.as_bytes();
    let pattern = pattern.as_bytes();

    let (tlen, plen) = (text.len(), pattern.len());
    let mut dp = vec![vec![false; plen + 1]; tlen + 1];
    dp[0][0] = true;

    // Handle leading *
    for j in 1..=plen {
        if pattern[j - 1] == b'*' {
            dp[0][j] = dp[0][j - 1];
        }
    }

    for i in 1..=tlen {
        for j in 1..=plen {
            if pattern[j - 1] == b'*' {
                dp[i][j] = dp[i - 1][j] || dp[i][j - 1];
            } else if pattern[j - 1] == b'?' || pattern[j - 1] == text[i - 1] {
                dp[i][j] = dp[i - 1][j - 1];
            }
        }
    }

    dp[tlen][plen]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_substring_match() {
        assert!(matches_query("report.pdf", "report"));
        assert!(matches_query("Report.PDF", "report"));
        assert!(!matches_query("report.pdf", "summary"));
    }

    #[test]
    fn test_glob_match() {
        assert!(matches_query("report.pdf", "*.pdf"));
        assert!(matches_query("report.pdf", "report.*"));
        assert!(matches_query("report.pdf", "repor?.pdf"));
        assert!(!matches_query("report.pdf", "*.txt"));
        assert!(matches_query("test.tar.gz", "*.tar.*"));
    }

    #[test]
    fn test_glob_star() {
        assert!(glob_match("hello", "*"));
        assert!(glob_match("hello", "h*o"));
        assert!(glob_match("hello", "h*"));
        assert!(!glob_match("hello", "h*x"));
    }
}
