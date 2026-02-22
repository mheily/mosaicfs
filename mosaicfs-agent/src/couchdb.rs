use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

#[derive(Clone)]
pub struct CouchClient {
    client: Client,
    base_url: String,
    db_name: String,
    auth: (String, String),
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
pub struct BulkDocsResponse(pub Vec<BulkDocResult>);

#[derive(Debug, Deserialize)]
pub struct BulkDocResult {
    pub ok: Option<bool>,
    pub id: Option<String>,
    pub rev: Option<String>,
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

#[derive(Debug, Deserialize)]
pub struct AllDocsResponse {
    pub total_rows: Option<u64>,
    pub rows: Vec<AllDocsRow>,
}

#[derive(Debug, Deserialize)]
pub struct AllDocsRow {
    pub id: String,
    pub key: String,
    pub value: serde_json::Value,
    pub doc: Option<serde_json::Value>,
}

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

    fn db_url(&self) -> String {
        format!("{}/{}", self.base_url, self.db_name)
    }

    /// Ensure the database exists, creating it if necessary
    pub async fn ensure_db(&self) -> Result<(), CouchError> {
        let resp = self
            .client
            .put(&self.db_url())
            .basic_auth(&self.auth.0, Some(&self.auth.1))
            .send()
            .await?;

        match resp.status().as_u16() {
            201 => {
                info!(db = %self.db_name, "Created database");
                Ok(())
            }
            412 => {
                info!(db = %self.db_name, "Database already exists");
                Ok(())
            }
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

    /// Fetch changes since a given sequence
    pub async fn changes(
        &self,
        since: &serde_json::Value,
        include_docs: bool,
    ) -> Result<ChangesResponse, CouchError> {
        let mut url = format!("{}/_changes?since={}", self.db_url(), since);
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

    /// Get all documents matching a key prefix (using _all_docs with startkey/endkey)
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
}
