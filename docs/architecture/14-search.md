<\!-- MosaicFS Architecture · ../architecture.md -->

## Search

### v1: Filename Search

In v1, search operates entirely on data already present in CouchDB — no additional indexing infrastructure is required. The search API endpoint accepts a query string and returns matching file documents. Two match modes are supported:

**Substring match** — returns all files where the `name` field contains the query string, case-insensitively. Suitable for quick lookups by partial filename.

**Glob match** — interprets the query as a glob pattern (`*.pdf`, `report-2025-*`, `**/*.rs`) and matches against `name`. Evaluated on the control plane against the CouchDB result set.

The search endpoint is:

```
GET /api/search?q=<query>&limit=100&offset=0
```

Results include `name`, `source.node_id`, `size`, and `mtime` for each match. Virtual paths are not included — a file may appear in multiple virtual directories and has no single canonical virtual path. The web UI search page and the CLI `mosaicfs-cli files search` command are both thin wrappers around this endpoint.

Because search is backed by CouchDB's existing file documents, it reflects the current state of the index in real time — new files appear in search results as soon as the agent indexes them and the document replicates to the control plane.

### Future: Richer Search

Filename search covers the most common retrieval need but leaves several useful capabilities for future versions:

**Metadata filtering** — constraining results by file type (MIME type or extension), size range, modification date range, or owning node. This requires no new infrastructure, only additional query parameters on the existing search endpoint and corresponding CouchDB Mango query expressions.

**Full-text content search** — searching inside the contents of documents, PDFs, source code, and other text-bearing file formats. This is a substantially larger undertaking: it requires fetching files from their owning nodes, running format-specific text extraction, and maintaining a dedicated search index. Tantivy (a Rust-native full-text search library) or Meilisearch are the candidate engines. Content search is out of scope for v1 but is the most significant planned search enhancement.

---

