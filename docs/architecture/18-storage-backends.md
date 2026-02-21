<\!-- MosaicFS Architecture · ../architecture.md -->

## Storage Backends (Cloud Services)

### Backend Interface

Each cloud storage backend plugin implements a common Rust trait with four methods: `list(path)` for directory contents, `stat(path)` for file metadata, `fetch(path)` returning an async byte stream, and `refresh_auth()` for token renewal. Backends are pluggable — adding a new cloud service means implementing this trait and registering a new backend type.

### Per-Service Notes

- **Google Drive** — Official REST API. OAuth2 with refresh tokens. Supports delta API for efficient incremental sync. Storage quota available via `storageQuota.limit` and `storageQuota.usage`.
- **Microsoft OneDrive** — Microsoft Graph API. Provides `quickXorHash`. OAuth2. Uses item IDs internally; the backend maintains a path-to-ID mapping. Storage quota available via `drive.quota`.
- **Backblaze B2** — S3-compatible API. `aws-sdk-rust` with custom endpoint. Object store model; directories are simulated from key prefixes. No quota ceiling; tracks used bytes only.
- **Amazon S3** — Native S3 API via `aws-sdk-rust`. Each bucket is registered as a separate storage backend. No quota ceiling; tracks used bytes only.
- **iCloud Drive** — No official third-party API. Accessed via the local `~/Library/Mobile Documents/` sync directory on macOS. Evicted files detected via the `com.apple.ubiquity.icloud-item-evicted` extended attribute and served via the control plane instead. Quota data not reliably available; `quota_available` is set to false.

### Polling Strategy

Google Drive and OneDrive support delta/changes APIs that return only what has changed since a stored cursor. These are polled frequently (every 60 seconds). Full listings are run periodically (every 5–10 minutes) as a consistency check. S3 and B2 have no push notification mechanism and rely on periodic full listings (every 10 minutes). Intervals are configurable per storage backend via `poll_interval_s` and `schedule` fields.

---

