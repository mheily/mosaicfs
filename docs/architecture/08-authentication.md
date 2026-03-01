<\!-- MosaicFS Architecture · ../architecture.md -->

## Authentication

### Credential Format

Access keys follow the AWS naming convention. The access key ID is a public identifier safe to include in logs. The secret key is shown once at creation time and stored only as an Argon2id hash.

```
Access Key ID:  MOSAICFS_7F3A9B2C1D4E5F6A   (public)
Secret Key:     mosaicfs_<43 url-safe base64 chars>  (shown once)
```

### Agent-to-Server: HMAC Request Signing

Agents authenticate to the control plane using HMAC-SHA256 request signing. The signed string is a canonical concatenation of the HTTP method, path, ISO 8601 timestamp, and SHA-256 body hash. Requests whose timestamp differs from the server's clock by more than 5 minutes in either direction are rejected to prevent replay attacks.

**Clock skew handling.** The 5-minute window accommodates typical NTP-synchronized clocks. Agents that fail authentication due to clock skew will see a persistent `401` error. The agent logs the server's `Date` response header alongside the local timestamp on authentication failures, making clock skew obvious in the logs. The agent does not automatically adjust its clock — clock management is the responsibility of the host OS (NTP, chrony, systemd-timesyncd). If an agent is consistently failing with timestamp errors, the notification system surfaces it via `notification::<node_id>::auth_timestamp_rejected`.

```
Authorization: MOSAICFS-HMAC-SHA256
  AccessKeyId=MOSAICFS_7F3A9B2C1D4E5F6A
  Timestamp=2025-11-14T09:22:00Z
  Signature=<hmac-sha256-hex>
```

### Web UI: JWT Sessions

The browser authenticates by presenting access key credentials to `POST /api/auth/login`. On success, the server issues a short-lived JWT (24-hour expiry) stored in memory — never in `localStorage`. All subsequent API requests include the JWT as a Bearer token.

**JWT signing key.** The JWT signing secret is a 256-bit random key generated at first control plane startup and stored in `jwt_secret` within the Docker Compose volume alongside the CouchDB data. The key is loaded into memory on startup and never exposed through the API. If the key is lost (volume destroyed), all existing JWTs become invalid — users must log in again, which is the correct behavior after a data loss event. v1 does not implement key rotation; the key is stable for the lifetime of the deployment. If rotation is needed in a future version, the server can accept tokens signed by both the current and previous key during a transition window.

### Download Tokens: Browser Media Embedding

Browser-native resource loading (`<video src>`, `<img src>`, `<iframe src>`, `<a href download>`) cannot attach an `Authorization: Bearer` header. To support inline media playback and native file downloads without buffering files through JavaScript, the control plane issues short-lived signed download tokens.

A client holding a valid JWT calls `GET /api/files/{file_id}/token`. The server issues a token and returns a `url` pointing to the public `GET /api/files/{file_id}/download?token=…` endpoint. That URL can be placed directly in a media element's `src` attribute.

**Token design.** Tokens are stateless — no server-side storage is required. Each token encodes an expiry timestamp and is HMAC-SHA256 signed with the server's JWT secret. Token format:

```
{expiry_unix_ts}.{hmac_hex}
HMAC message: "dl:{doc_id}:{expiry_unix_ts}"
```

Validation is constant-time to prevent timing attacks. Tokens expire after **2 hours** — long enough for a video streaming session to complete without requiring a token refresh. Tokens are scoped to a single file ID; a token for one file cannot be used to access another.

**Security properties.** The token-authenticated download endpoint is public (no JWT middleware), but a valid token can only be obtained by an authenticated user via the JWT-protected `/token` endpoint. The effective security is: "any user who can log in can generate a time-limited URL for any file, and that URL is valid for 2 hours." This matches the expected threat model for a self-hosted private system.

### Agent-to-Agent: Credential Presentation

When one agent requests a file from another agent's transfer server, it presents its own access key ID and a HMAC-signed request. The receiving agent validates against its local credential store — which is kept current via CouchDB replication — so transfer authentication works even if the control plane is temporarily unreachable.

---

