//! Lightweight CouchDB client for VFS operations.
//!
//! Duplicates the minimal subset needed by the VFS layer rather than
//! depending on the server or agent crate.

use serde::Deserialize;

#[derive(Debug, thiserror::Error)]
pub enum CouchError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("CouchDB error: {error} — {reason}")]
    Couch { error: String, reason: String },
    #[error("Not found: {0}")]
    NotFound(String),
}

#[derive(Debug, Deserialize)]
pub struct CouchResponse {
    pub ok: Option<bool>,
    pub error: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AllDocsResponse {
    pub total_rows: Option<u64>,
    pub rows: Vec<AllDocsRow>,
}

#[derive(Debug, Deserialize)]
pub struct AllDocsRow {
    pub id: String,
    pub key: String,
    pub doc: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct FindResponse {
    pub docs: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct BulkDocResult {
    pub id: Option<String>,
    pub ok: Option<bool>,
    pub error: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ChangesResponse {
    pub results: Vec<ChangeRow>,
    pub last_seq: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct ChangeRow {
    pub seq: serde_json::Value,
    pub id: String,
    pub changes: Vec<ChangeRev>,
    #[serde(default)]
    pub deleted: bool,
    pub doc: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct ChangeRev {
    pub rev: String,
}

#[derive(Clone)]
pub struct CouchClient {
    pub client: reqwest::Client,
    pub base_url: String,
    pub db_name: String,
    pub auth: (String, String),
}

impl CouchClient {
    pub fn new(base_url: &str, db_name: &str, user: &str, password: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            db_name: db_name.to_string(),
            auth: (user.to_string(), password.to_string()),
        }
    }

    pub fn from_env(db_name: &str) -> Self {
        let url = std::env::var("COUCHDB_URL").unwrap_or_else(|_| "http://localhost:5984".into());
        let user = std::env::var("COUCHDB_USER").unwrap_or_else(|_| "admin".into());
        let pass = std::env::var("COUCHDB_PASSWORD").unwrap_or_else(|_| "password".into());
        Self::new(&url, db_name, &user, &pass)
    }

    pub fn db_url(&self) -> String {
        format!("{}/{}", self.base_url, self.db_name)
    }

    pub async fn get_document(&self, doc_id: &str) -> Result<serde_json::Value, CouchError> {
        let resp = self
            .client
            .get(format!("{}/{}", self.db_url(), urlencoding::encode(doc_id)))
            .basic_auth(&self.auth.0, Some(&self.auth.1))
            .send()
            .await?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(CouchError::NotFound(doc_id.to_string()));
        }
        if resp.status().is_success() {
            Ok(resp.json().await?)
        } else {
            let body: CouchResponse = resp.json().await?;
            Err(CouchError::Couch {
                error: body.error.unwrap_or_default(),
                reason: body.reason.unwrap_or_default(),
            })
        }
    }

    pub async fn all_docs_by_prefix(
        &self,
        prefix: &str,
        include_docs: bool,
    ) -> Result<AllDocsResponse, CouchError> {
        let end_key = format!("{}\u{ffff}", prefix);
        let resp = self
            .client
            .get(format!("{}/_all_docs", self.db_url()))
            .basic_auth(&self.auth.0, Some(&self.auth.1))
            .query(&[
                ("startkey", format!("\"{}\"", prefix)),
                ("endkey", format!("\"{}\"", end_key)),
                ("include_docs", include_docs.to_string()),
            ])
            .send()
            .await?;

        if resp.status().is_success() {
            Ok(resp.json().await?)
        } else {
            let body: CouchResponse = resp.json().await?;
            Err(CouchError::Couch {
                error: body.error.unwrap_or_default(),
                reason: body.reason.unwrap_or_default(),
            })
        }
    }

    pub async fn find(&self, selector: serde_json::Value) -> Result<FindResponse, CouchError> {
        let resp = self
            .client
            .post(format!("{}/_find", self.db_url()))
            .basic_auth(&self.auth.0, Some(&self.auth.1))
            .json(&serde_json::json!({ "selector": selector, "limit": 10000 }))
            .send()
            .await?;

        if resp.status().is_success() {
            Ok(resp.json().await?)
        } else {
            let body: CouchResponse = resp.json().await?;
            Err(CouchError::Couch {
                error: body.error.unwrap_or_default(),
                reason: body.reason.unwrap_or_default(),
            })
        }
    }

    pub async fn bulk_docs(
        &self,
        docs: &[serde_json::Value],
    ) -> Result<Vec<BulkDocResult>, CouchError> {
        let resp = self
            .client
            .post(format!("{}/_bulk_docs", self.db_url()))
            .basic_auth(&self.auth.0, Some(&self.auth.1))
            .json(&serde_json::json!({ "docs": docs }))
            .send()
            .await?;

        if resp.status().is_success() {
            Ok(resp.json().await?)
        } else {
            let body: CouchResponse = resp.json().await?;
            Err(CouchError::Couch {
                error: body.error.unwrap_or_default(),
                reason: body.reason.unwrap_or_default(),
            })
        }
    }

    pub async fn changes(
        &self,
        since: &serde_json::Value,
        limit: u64,
        include_docs: bool,
    ) -> Result<ChangesResponse, CouchError> {
        let since_str = match since {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Number(n) => n.to_string(),
            _ => "0".to_string(),
        };
        let mut query = vec![
            ("since".to_string(), since_str),
            ("limit".to_string(), limit.to_string()),
        ];
        if include_docs {
            query.push(("include_docs".to_string(), "true".to_string()));
        }
        let resp = self
            .client
            .get(format!("{}/_changes", self.db_url()))
            .basic_auth(&self.auth.0, Some(&self.auth.1))
            .query(&query)
            .send()
            .await?;

        if resp.status().is_success() {
            Ok(resp.json().await?)
        } else {
            let body: CouchResponse = resp.json().await?;
            Err(CouchError::Couch {
                error: body.error.unwrap_or_default(),
                reason: body.reason.unwrap_or_default(),
            })
        }
    }
}
