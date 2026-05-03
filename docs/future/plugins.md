Frozen at commit e44e1de. Source files listed below; recover with git show e44e1de:<path>.

Recovery files:
- `mosaicfs-common/src/documents.rs` (PluginDocument, PluginType, QueryEndpoint, default_workers, default_timeout, default_max_attempts, test_plugin_document)
- `mosaicfs-server/src/routes.rs` (plugin routes)

---

# Future: Plugins

## Intent

The plugin system was designed to let external processes annotate files
with metadata produced by specialized tools (OCR, EXIF extraction, virus
scanning, etc.). A plugin would register with a node, subscribe to file
events matching its `mime_globs`, and be invoked by the node's plugin
runner on new or updated files. Plugins could also expose query endpoints
so the VFS step pipeline could filter files based on plugin output (e.g.,
"only include files where the OCR plugin found the word 'invoice'").
Plugins and annotations are coupled: reintroduce both together when there
is a concrete first plugin to ship.

## Struct definitions

```rust
// ── Plugin Document ──

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PluginDocument {
    #[serde(rename = "type")]
    pub doc_type: PluginType,
    pub node_id: String,
    pub plugin_name: String,
    pub plugin_type: String,
    pub enabled: bool,
    pub name: String,
    #[serde(default)]
    pub subscribed_events: Vec<String>,
    #[serde(default)]
    pub mime_globs: Vec<String>,
    #[serde(default)]
    pub config: serde_json::Value,
    #[serde(default = "default_workers")]
    pub workers: i32,
    #[serde(default = "default_timeout")]
    pub timeout_s: i32,
    #[serde(default = "default_max_attempts")]
    pub max_attempts: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query_endpoints: Option<Vec<QueryEndpoint>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settings_schema: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settings: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provides_filesystem: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path_prefix: Option<String>,
    pub created_at: DateTime<Utc>,
}

fn default_workers() -> i32 { 2 }
fn default_timeout() -> i32 { 60 }
fn default_max_attempts() -> i32 { 3 }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PluginType {
    #[serde(rename = "plugin")]
    Plugin,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QueryEndpoint {
    pub name: String,
    pub capability: String,
    pub description: String,
}
```

## Route shapes

All six plugin routes were stubs returning `501 Not Implemented`:

```
GET    /api/nodes/{node_id}/plugins
POST   /api/nodes/{node_id}/plugins
GET    /api/nodes/{node_id}/plugins/{plugin_name}
PATCH  /api/nodes/{node_id}/plugins/{plugin_name}
DELETE /api/nodes/{node_id}/plugins/{plugin_name}
POST   /api/nodes/{node_id}/plugins/{plugin_name}/sync
```

## CouchDB document ID scheme

`plugin::<node_id>::<plugin_name>`

## SQLite schema (from change 016 design-notes §4)

The future SQL schema for plugins (see `docs/changes/016/design-notes.md` §4
for the full context) used `(node_id, plugin_name)` as a composite primary key,
replacing the synthetic CouchDB id.
