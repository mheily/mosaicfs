<\!-- MosaicFS Architecture · ../architecture.md -->

## Security

### Threat Model

MosaicFS is designed for a single owner operating a private home network. The relevant threats are an attacker on the same local network attempting to intercept or access file data, and accidental exposure of the control plane to the internet. It is not designed to defend against a malicious device owner — an attacker with physical access to a machine running an agent is outside the threat model. It is also not a multi-user system in v1; there is no concept of one user's files being hidden from another.

### Trust Boundaries

There are four trust boundaries in the system:

**The control plane** is the most trusted component. It holds the authoritative database, manages credentials, and stores storage backend tokens. It should run on a machine you physically control and trust — a home NAS or a private cloud instance. Nothing in the system should be more exposed than the control plane.

**Physical agents** are trusted once they have presented a valid credential. An agent that has been issued a key can read any file in the system, push documents into the global index, and authenticate with other agents. A compromised agent is a meaningful threat — it can exfiltrate any file it can reach. Credential revocation via the control plane immediately cuts off a compromised agent from the control plane and from other agents (whose local credential replicas will update within minutes).

**Storage backend credentials** are stored in CouchDB as credential documents and replicated only to the agent(s) that need them. OAuth tokens for cloud services are stored in encrypted files on the host running the backend's hosting agent and are not replicated to other agents.

**Clients** — the web UI, CLI, and desktop app — are trusted to the extent their credential allows. The CLI and desktop app use HMAC-signed access key credentials, the same mechanism as agents. The web UI uses a restricted read-only CouchDB session for live data sync (see below) plus JWT-authenticated REST API calls for mutations. A leaked CLI credential grants the same broad access as an agent credential; a hijacked browser session is limited to read access on the database.

### What the Design Provides

- **TLS on all external connections.** The control plane generates a self-signed CA and server certificate at setup time. Agents and clients verify the server certificate against this CA. All traffic between clients and the control plane is encrypted in transit.
- **HMAC-signed requests prevent replay attacks.** Agent requests include a timestamp; the control plane rejects requests with a timestamp older than five minutes. An intercepted request cannot be replayed after that window.
- **Credentials stored as Argon2id hashes.** Secret keys are hashed with Argon2id on first presentation and never stored in recoverable form. A database dump does not expose usable credentials.
- **CouchDB bound to localhost only.** CouchDB is not directly reachable from the network. The Axum server is the only externally-accessible process. Agent-to-CouchDB replication runs through an Axum-proxied endpoint authenticated with HMAC credentials. Browser clients do not use this proxy — instead, the Axum login endpoint issues PouchDB a short-lived session token for a restricted CouchDB user (`mosaicfs_browser`) that has read-only access to a scoped subset of the database. Push attempts from the browser are rejected by CouchDB's own permission model, not by filter logic. This means a hijacked browser session cannot modify rules, disable credentials, or corrupt the index — the worst it can do is read documents the browser filter allows.
- **Agent-to-agent transfers are authenticated.** Transfer requests between agents use the same HMAC signing as agent-to-control-plane requests, validated against the local credential replica. The control plane does not need to be reachable for P2P transfers to be authenticated.
- **Secret keys are never logged or passed as CLI arguments.** The agent init command reads the secret key from stdin with echo disabled. The `MOSAICFS_SECRET_KEY` environment variable is available for scripted deployments, but the key is never accepted as a positional argument that would appear in shell history or process listings.

### Secret Storage at Rest

| Location | What is stored | How |
|---|---|---|
| Control plane host | CouchDB admin credential | Docker Compose environment file, readable only by the compose service user |
| Backend hosting agent | Storage backend OAuth tokens | Encrypted files in `storage-backends/`, key derived from a host secret at startup |
| Agent host | Agent access key ID and secret | `agent.toml`, file permissions `0600`, owned by the agent service user |
| CLI user machine | CLI access key ID and secret | `~/.config/mosaicfs/cli.toml`, file permissions `0600` |
| Browser | Web UI session JWT (for REST API calls) | In-memory only — never written to `localStorage` or cookies |
| Browser | PouchDB session token for `mosaicfs_browser` CouchDB user | In-memory only, short-lived, read-only scope |

### Network Exposure

The control plane exposes one port externally: the Axum HTTPS API server (default 8443). CouchDB listens on localhost only and is not directly reachable from outside the host. Agent-to-CouchDB replication runs through an Axum-proxied endpoint, authenticated with HMAC credentials before the connection is passed through. Browser clients connect to CouchDB directly via PouchDB using a short-lived session token for the read-only `mosaicfs_browser` CouchDB user, issued by Axum on successful login. Agents expose one port for P2P file transfers (default 7845), which should be accessible only within the local network.

For deployments where the control plane needs to be reachable from outside the home network — to support the web UI or CLI from a remote location — the recommended approach is a VPN (Tailscale is a natural fit) rather than exposing port 8443 directly to the internet. If direct internet exposure is unavoidable, the control plane should be placed behind a reverse proxy with rate limiting and, ideally, IP allowlisting.

### Known Gaps and Multi-User Considerations

The v1 security model makes several deliberate simplifications that would need to be revisited before MosaicFS could support multiple users with private, isolated file namespaces:

**Flat credential permissions.** Every credential grants full access to the entire system. The `permissions.scope` field in the credential document is reserved for future use but has no effect in v1. Adding per-user access control requires a permission model that the VFS layer, rule engine, transfer endpoints, and every API route would need to enforce.

**Global virtual directories.** All virtual directories and their mount configurations are shared across all credentials. In a multi-user system, different users would need different virtual trees. Adding per-user directory ownership is feasible in the schema, but requires changes to the rule engine (evaluate only directories the credential owns) and to the replication filters (agents replicate only the directory documents relevant to them).

**CouchDB replication is not per-credential.** Agents replicate directly with CouchDB using a shared internal credential managed by the control plane. The replication filter controls what documents travel, but any agent with replication access can pull any document that passes the filter. Proper per-user isolation would likely require moving agent synchronization behind the Axum API, which can enforce credential-scoped access — a meaningful architectural change from direct CouchDB replication.

**File transfers have no per-file authorization.** Any valid credential can request any file from any agent's transfer server. The transfer endpoint authenticates the caller but does not check whether that caller is permitted to access the specific file requested.

None of these are blockers for a single-user deployment. The credential schema, rule document structure, and node ownership patterns are designed to accommodate these extensions, but the work of implementing them is substantial and is deferred to a future version.

---

