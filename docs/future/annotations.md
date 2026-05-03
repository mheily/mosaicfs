Frozen at commit e44e1de. Source files listed below; recover with git show e44e1de:<path>.

Recovery files:
- `mosaicfs-common/src/documents.rs` (AnnotationDocument, AnnotationType, AnnotationSource, test_annotation_document)
- `mosaicfs-server/src/routes.rs` (annotation routes)

---

# Future: Annotations

## Intent

Annotations are how plugins surface their results back into the file index.
When a plugin finishes processing a file, it writes an `AnnotationDocument`
keyed by `(file_id, plugin_name)` that carries the plugin's output as
free-form JSON plus a status (pending / done / error). The VFS step pipeline
could filter using annotations via an `annotation` step op that checks whether
a named plugin's annotation matches expected conditions. Annotations are
dependent on plugins — reintroduce both together when there is a concrete
first plugin to ship.

## Struct definitions

```rust
// ── Annotation Document ──

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnnotationDocument {
    #[serde(rename = "type")]
    pub doc_type: AnnotationType,
    pub file_id: String,
    pub source: AnnotationSource,
    pub plugin_name: String,
    pub data: serde_json::Value,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub annotated_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AnnotationType {
    #[serde(rename = "annotation")]
    Annotation,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnnotationSource {
    pub node_id: String,
}
```

## Route shapes

Both annotation routes were stubs returning `501 Not Implemented`:

```
GET    /api/annotations
DELETE /api/annotations
```

## CouchDB document ID scheme

`annotation::<file_uuid>::<plugin_name>`

## VFS step op

The `annotation` step op accepted a `plugin_name` parameter and was handled
in `mosaicfs-server/src/ui/actions.rs` (`build_step_json`) and referenced in
`mosaicfs-server/src/ui/views.rs` (`step_params_summary`). Those handlers
still contain the dead branches (harmless; no annotation documents exist).

## SQLite schema (from change 016 design-notes §4)

```sql
CREATE TABLE annotation (
    file_origin_node_id INTEGER NOT NULL,
    file_origin_file_id INTEGER NOT NULL,
    plugin_name TEXT NOT NULL,
    origin_node_id INTEGER NOT NULL,     -- the annotating node
    data TEXT NOT NULL,                  -- JSON
    status TEXT NOT NULL,
    error TEXT,
    annotated_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (file_origin_node_id, file_origin_file_id, plugin_name)
);
CREATE INDEX idx_annotation_file ON annotation(file_origin_node_id, file_origin_file_id);
```
