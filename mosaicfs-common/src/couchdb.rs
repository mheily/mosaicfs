//! Shared CouchDB HTTP client used by all MosaicFS crates.
//!
//! Consolidates the minimal set of CouchDB operations needed across the VFS,
//! agent, and server binaries into a single implementation. Each binary used
//! to carry its own copy with slightly different signatures and drifted bug
//! fixes; this module is the canonical home.

use reqwest::Client;
use serde::Deserialize;
use tracing::info;

#[derive(Debug, thiserror::Error)]
pub enum CouchError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("CouchDB error: {error} - {reason}")]
    Couch { error: String, reason: String },
    #[error("Document not found: {0}")]
    NotFound(String),
    #[error("Conflict: {0}")]
    Conflict(String),
}

#[derive(Debug, Deserialize)]
pub struct CouchResponse {
    pub ok: Option<bool>,
    pub id: Option<String>,
    pub rev: Option<String>,
    pub error: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct BulkDocResult {
    pub ok: Option<bool>,
    pub id: Option<String>,
    pub rev: Option<String>,
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
    #[serde(default)]
    pub value: serde_json::Value,
    pub doc: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct ChangesResponse {
    pub results: Vec<ChangeRow>,
    pub last_seq: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct ChangeRow {
    #[serde(default)]
    pub seq: serde_json::Value,
    pub id: String,
    #[serde(default)]
    pub changes: Vec<ChangeRev>,
    #[serde(default)]
    pub deleted: bool,
    pub doc: Option<serde_json::Value>,
}

impl ChangesResponse {
    /// Render `last_seq` as a string suitable for the next `since=` query.
    pub fn last_seq_string(&self) -> String {
        match &self.last_seq {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Number(n) => n.to_string(),
            v => v.to_string(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct ChangeRev {
    pub rev: String,
}

#[derive(Debug, Deserialize)]
pub struct FindResponse {
    pub docs: Vec<serde_json::Value>,
}

#[derive(Clone)]
pub struct CouchClient {
    pub client: Client,
    pub base_url: String,
    pub db_name: String,
    pub auth: (String, String),
}

impl CouchClient {
    pub fn new(base_url: &str, db_name: &str, user: &str, password: &str) -> Self {
        Self {
            client: Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            db_name: db_name.to_string(),
            auth: (user.to_string(), password.to_string()),
        }
    }

    pub fn from_env(db_name: &str) -> Self {
        let url = std::env::var("COUCHDB_URL").expect("COUCHDB_URL must be set");
        let user = std::env::var("COUCHDB_USER").expect("COUCHDB_USER must be set");
        let password = std::env::var("COUCHDB_PASSWORD").expect("COUCHDB_PASSWORD must be set");
        Self::new(&url, db_name, &user, &password)
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn auth(&self) -> &(String, String) {
        &self.auth
    }

    pub fn db_url(&self) -> String {
        format!("{}/{}", self.base_url, self.db_name)
    }

    pub async fn ensure_db(&self) -> Result<(), CouchError> {
        let resp = self
            .client
            .put(self.db_url())
            .basic_auth(&self.auth.0, Some(&self.auth.1))
            .send()
            .await?;

        match resp.status().as_u16() {
            201 => {
                info!(db = %self.db_name, "Created database");
                Ok(())
            }
            412 => Ok(()),
            status => {
                let body: CouchResponse = resp.json().await?;
                Err(CouchError::Couch {
                    error: body.error.unwrap_or_else(|| format!("HTTP {status}")),
                    reason: body.reason.unwrap_or_default(),
                })
            }
        }
    }

    pub async fn get_document(&self, id: &str) -> Result<serde_json::Value, CouchError> {
        let resp = self
            .client
            .get(format!("{}/{}", self.db_url(), urlencoding::encode(id)))
            .basic_auth(&self.auth.0, Some(&self.auth.1))
            .send()
            .await?;

        match resp.status().as_u16() {
            200 => Ok(resp.json().await?),
            404 => Err(CouchError::NotFound(id.to_string())),
            _ => {
                let body: CouchResponse = resp.json().await?;
                Err(CouchError::Couch {
                    error: body.error.unwrap_or_default(),
                    reason: body.reason.unwrap_or_default(),
                })
            }
        }
    }

    pub async fn put_document(
        &self,
        id: &str,
        doc: &serde_json::Value,
    ) -> Result<CouchResponse, CouchError> {
        let resp = self
            .client
            .put(format!("{}/{}", self.db_url(), urlencoding::encode(id)))
            .basic_auth(&self.auth.0, Some(&self.auth.1))
            .json(doc)
            .send()
            .await?;

        match resp.status().as_u16() {
            201 | 202 => Ok(resp.json().await?),
            409 => Err(CouchError::Conflict(id.to_string())),
            _ => {
                let body: CouchResponse = resp.json().await?;
                Err(CouchError::Couch {
                    error: body.error.unwrap_or_default(),
                    reason: body.reason.unwrap_or_default(),
                })
            }
        }
    }

    pub async fn delete_document(&self, id: &str, rev: &str) -> Result<CouchResponse, CouchError> {
        let resp = self
            .client
            .delete(format!(
                "{}/{}?rev={}",
                self.db_url(),
                urlencoding::encode(id),
                urlencoding::encode(rev),
            ))
            .basic_auth(&self.auth.0, Some(&self.auth.1))
            .send()
            .await?;

        match resp.status().as_u16() {
            200 | 202 => Ok(resp.json().await?),
            _ => {
                let body: CouchResponse = resp.json().await?;
                Err(CouchError::Couch {
                    error: body.error.unwrap_or_default(),
                    reason: body.reason.unwrap_or_default(),
                })
            }
        }
    }

    pub async fn bulk_docs(
        &self,
        docs: &[serde_json::Value],
    ) -> Result<Vec<BulkDocResult>, CouchError> {
        let payload = serde_json::json!({ "docs": docs });
        let resp = self
            .client
            .post(format!("{}/_bulk_docs", self.db_url()))
            .basic_auth(&self.auth.0, Some(&self.auth.1))
            .json(&payload)
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
        since: &str,
        include_docs: bool,
        limit: Option<u64>,
    ) -> Result<ChangesResponse, CouchError> {
        let mut query: Vec<(String, String)> = vec![("since".to_string(), since.to_string())];
        if include_docs {
            query.push(("include_docs".to_string(), "true".to_string()));
        }
        if let Some(l) = limit {
            query.push(("limit".to_string(), l.to_string()));
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

    pub async fn all_docs_by_prefix(
        &self,
        prefix: &str,
        include_docs: bool,
    ) -> Result<AllDocsResponse, CouchError> {
        let startkey = serde_json::to_string(prefix).unwrap();
        let endkey = serde_json::to_string(&format!("{}\u{ffff}", prefix)).unwrap();
        let mut url = format!(
            "{}/_all_docs?startkey={}&endkey={}",
            self.db_url(),
            urlencoding::encode(&startkey),
            urlencoding::encode(&endkey),
        );
        if include_docs {
            url.push_str("&include_docs=true");
        }
        let resp = self
            .client
            .get(&url)
            .basic_auth(&self.auth.0, Some(&self.auth.1))
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

    pub async fn all_docs(&self, include_docs: bool) -> Result<AllDocsResponse, CouchError> {
        let mut url = format!("{}/_all_docs", self.db_url());
        if include_docs {
            url.push_str("?include_docs=true");
        }
        let resp = self
            .client
            .get(&url)
            .basic_auth(&self.auth.0, Some(&self.auth.1))
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

    pub async fn db_info(&self) -> Result<serde_json::Value, CouchError> {
        let resp = self
            .client
            .get(self.db_url())
            .basic_auth(&self.auth.0, Some(&self.auth.1))
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

    pub async fn delete_db(&self) -> Result<(), CouchError> {
        let resp = self
            .client
            .delete(self.db_url())
            .basic_auth(&self.auth.0, Some(&self.auth.1))
            .send()
            .await?;

        if resp.status().is_success() {
            info!(db = %self.db_name, "Deleted database");
            Ok(())
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
}

/// Create the Mango indexes the server relies on for typed queries.
pub async fn create_indexes(db: &CouchClient) -> Result<(), CouchError> {
    let indexes = vec![
        ("idx-type", serde_json::json!({"fields": ["type"]})),
        ("idx-type-status", serde_json::json!({"fields": ["type", "status"]})),
        ("idx-type-node", serde_json::json!({"fields": ["type", "source.node_id"]})),
        ("idx-type-name", serde_json::json!({"fields": ["type", "name"]})),
        ("idx-credential-akid", serde_json::json!({"fields": ["type", "access_key_id"]})),
        ("idx-file-export-parent", serde_json::json!({"fields": ["type", "source.export_parent"]})),
        ("idx-label-rule-node", serde_json::json!({"fields": ["type", "node_id", "enabled"]})),
    ];

    for (name, index) in indexes {
        let payload = serde_json::json!({
            "index": index,
            "ddoc": name,
            "name": name,
            "type": "json",
        });

        let resp = db
            .client
            .post(format!("{}/_index", db.db_url()))
            .basic_auth(&db.auth.0, Some(&db.auth.1))
            .json(&payload)
            .send()
            .await?;

        if !resp.status().is_success() {
            let body: CouchResponse = resp.json().await?;
            tracing::warn!(
                index = name,
                error = ?body.error,
                reason = ?body.reason,
                "Failed to create index"
            );
        }
    }

    Ok(())
}
