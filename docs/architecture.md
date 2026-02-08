# MosaicFS Architecture

## Executive Summary

This document outlines the design for MosaicFS

## Who This Is For

MosaicFS is designed for home users who have data scattered across multiple devices and want to bring it all together into one unified view.

### The Typical Setup

Picture someone with:
- A **laptop** they carry around, with important files on its internal drive
- A **desktop PC** at home, with more files on its internal drive
- One or two **NAS devices** on the home network (maybe a Synology or QNAP)
- Some older files archived in **Amazon S3** (or similar cloud storage)

Right now, finding a specific file means remembering which device has it. Backing up means copying files manually between devices. And when the laptop's drive fills up, there's no easy way to move old files to the NAS without breaking bookmarks and scripts that expect files to be in certain places.

### What MosaicFS Does

MosaicFS gives you a single filesystem that spans all your storage:

```
/global/
├── documents/          ← might live on your NAS
├── photos/             ← some on NAS, old ones in S3
├── projects/           ← on your laptop's local drive
└── archive/            ← in S3 cold storage
```

You access everything through `/global/` regardless of where the data actually lives. The system handles:

- **Finding files**: metadata is replicated to every device, so `ls` and `find` are instant even when offline
- **Reading files**: data is fetched from wherever it lives (local drive, NAS, or cloud)
- **Auto-migration**: old, unused files can automatically move to cheaper storage (like S3), while frequently-accessed files stay on fast local drives

### Assumptions About Your Network

MosaicFS assumes you've already set up file sharing between your devices using standard tools:

1. **NFS or SMB/CIFS mounts**: Your laptop and PC can already access your NAS devices through normal network mounts. You've configured authentication (passwords, Kerberos, whatever your NAS uses) through your operating system's standard tools.

2. **S3 credentials**: For cloud storage, you have AWS credentials (or equivalent) configured on each machine.

3. **Local network access**: Your devices can reach each other when on the same network. mosaicfs handles offline operation gracefully when they can't.

MosaicFS doesn't try to reinvent file sharing authentication. Instead, it builds on top of the mounts you've already configured. You tell mosaicfs where each data plane is mounted on each machine, and it handles the rest.

---

## Architecture Overview

### High-Level Design

```
                     ┌─────────────────────────────────────┐
                     │          CONTROL PLANE               │
                     │  ┌─────────────┐  ┌──────────────┐  │
                     │  │   CouchDB   │◄─│  API Server  │  │
                     │  │  (metadata) │  │  (writes)    │  │
                     │  └──────┬──────┘  └──────────────┘  │
                     └─────────┼───────────────────────────┘
                               │
            ┌──────────────────┼──────────────────┐
            │   Continuous     │                  │
            │   replication    │                  │
            ▼                  ▼                  ▼
   ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐
   │     LAPTOP      │  │    DESKTOP      │  │   NAS DEVICE    │
   │ ┌─────────────┐ │  │ ┌─────────────┐ │  │                 │
   │ │Local CouchDB│ │  │ │Local CouchDB│ │  │  Runs control   │
   │ └─────────────┘ │  │ └─────────────┘ │  │  plane + stores │
   │ ┌─────────────┐ │  │ ┌─────────────┐ │  │  shared files   │
   │ │ FUSE client │ │  │ │ FUSE client │ │  │                 │
   │ └─────────────┘ │  │ └─────────────┘ │  └─────────────────┘
   │                 │  │                 │           ▲
   │  Data planes:   │  │  Data planes:   │           │
   │  - local SSD    │  │  - local HDD    │     NFS/SMB mounts
   │  - NAS (mount)  │  │  - NAS (mount)  │           │
   │  - S3 (rclone)  │  │  - S3 (rclone)  │           │
   └────────┬────────┘  └────────┬────────┘           │
            │                    │                    │
            └────────────────────┴────────────────────┘
```

**Key insight:** Clients access NAS data through standard OS mounts (NFS/SMB), not through HTTP.
The user configures these mounts once using their OS tools, then tells mosaicfs where to find them.

### Control Plane vs. Data Plane

**Control Plane (runs on your NAS or always-on device):**
- CouchDB database holding all filesystem metadata
- Rust API server for write operations and coordination
- Replicates metadata to all your devices via CouchDB continuous replication
- Handles auto-migration decisions (which files go where)

**Data Planes (your actual storage):**
- **Local drives:** Your laptop's SSD, your desktop's HDD—accessed directly
- **NAS devices:** Accessed via NFS or SMB mounts you've already configured
- **Cloud storage:** S3 or similar, accessed via AWS SDK or rclone mount
- Each device has its own configuration mapping data plane IDs to local paths

---

## Protocol Design: RESTful JSON over HTTP/2

### Protocol Version

URL structure: `/api/v8/<operation>/<path>`

**Version in URL provides:**
- Clear upgrade path (run v7 and v8 side-by-side)
- Client/server protocol negotiation
- Backward compatibility during migration

### Transport: HTTP/2

**Why HTTP/2 (not HTTP/3):**
- Mature ecosystem (libraries, proxies, tools)
- Universal caching proxy support (Varnish, nginx)
- Proven at scale with mosaicfs's existing architecture
- HTTP/3 consideration deferred until proxy ecosystem matures (~2026-2027)

### Response Format: JSON

**Standard response structure:**
```json
{
  "status": "success",
  "errno": 0,
  "data": {
    "mode": 33188,
    "size": 1024,
    "mtime": 1234567890
  },
  "metadata": {
    "estalecookie": 1234567890123456,
    "validator": 999
  },
  "storage": {
    "backend": "nas-01",
    "url": "http://nas-01:8080/data/path/to/file",
    "tier": "hot"
  },
  "cache": {
    "max_age": 86400,
    "stale_while_revalidate": 30
  }
}
```

**Error responses:**
```json
{
  "status": "error",
  "errno": 2,
  "error": "ENOENT: No such file or directory",
  "path": "/nonexistent/file"
}
```

---

## Core Operations

### Read-Only Operations

| Operation | Endpoint | Description |
|-----------|----------|-------------|
| **stat** | `GET /api/v8/stat/<path>` | File attributes |
| **readdir** | `GET /api/v8/readdir/<path>` | Directory listing |
| **read** | `GET /api/v8/read/<path>?offset=N&length=M` | File content |
| **readlink** | `GET /api/v8/readlink/<path>` | Symlink target |
| **getxattr** | `GET /api/v8/xattr/<path>?name=X` | Extended attribute |
| **listxattr** | `GET /api/v8/xattr/<path>` | List all xattrs |
| **statfs** | `GET /api/v8/statfs/<path>` | Filesystem stats |

**Example: stat operation**
```http
GET /api/v8/stat/data/file.txt HTTP/2
Host: control.mosaicfs.example.com

Response:
{
  "status": "success",
  "errno": 0,
  "data": {
    "mode": 33188,
    "nlink": 1,
    "uid": 1000,
    "gid": 1000,
    "size": 4096,
    "atime": 1234567890,
    "mtime": 1234567890,
    "ctime": 1234567890
  },
  "metadata": {
    "estalecookie": 1570212575206024942,
    "validator": 746338405
  },
  "cache": {
    "max_age": 86400
  }
}
```

### Write Operations (New in v8)

Write operations use a **write-once, open/write/close model** for simplicity.

**Key constraints:**
- Can only create new files (not modify existing)
- Single writer per file
- File invisible to other clients until close
- No concurrent access or modifications after close

#### File Creation Flow

**1. Open (create write session)**
```http
POST /api/v8/open
Content-Type: application/json

{
  "path": "/data/newfile.txt",
  "mode": 0644,
  "flags": ["O_CREAT", "O_EXCL", "O_WRONLY"],
  "hints": {
    "size_estimate": 1048576,
    "tier": "hot"
  }
}

Response:
{
  "status": "success",
  "session_id": "550e8400-e29b-41d4-a716-446655440000",
  "upload_url": "/api/v8/upload/550e8400-...",
  "target_backend": "nas-01",
  "expires_at": 1234567890
}
```

**2. Write (one or more chunks)**
```http
PUT /api/v8/upload/{session_id}
Content-Type: application/octet-stream
Content-Length: 4096

[binary data]

Response:
{
  "status": "success",
  "bytes_written": 4096,
  "total_size": 4096
}
```

**3. Close (atomic publish)**
```http
POST /api/v8/close
Content-Type: application/json

{
  "session_id": "550e8400-e29b-41d4-a716-446655440000"
}

Response:
{
  "status": "success",
  "path": "/data/newfile.txt",
  "size": 4096,
  "estalecookie": 1234567890123456,
  "validator": 1000,
  "storage": {
    "backend": "nas-01",
    "url": "http://nas-01:8080/data/newfile.txt"
  }
}
```

**Session lifecycle:**
- Session created in CouchDB with temporary status
- Writes buffered in control plane or streamed to data plane
- Close commits metadata atomically
- Failed sessions cleaned up after timeout (1 hour default)

### Additional FUSE Operations

| Operation | Endpoint | Method | Description |
|-----------|----------|--------|-------------|
| **mkdir** | `/api/v8/mkdir` | POST | Create directory |
| **rmdir** | `/api/v8/rmdir` | DELETE | Remove empty directory |
| **unlink** | `/api/v8/unlink` | DELETE | Delete file |
| **symlink** | `/api/v8/symlink` | POST | Create symbolic link |
| **link** | `/api/v8/link` | POST | Create hard link |
| **rename** | `/api/v8/rename` | POST | Atomic rename (server-side) |
| **chmod** | `/api/v8/chmod` | PATCH | Change permissions |
| **chown** | `/api/v8/chown` | PATCH | Change ownership |
| **utimens** | `/api/v8/utimens` | PATCH | Update timestamps |
| **setxattr** | `/api/v8/xattr/<path>` | PUT | Set extended attribute |
| **removexattr** | `/api/v8/xattr/<path>` | DELETE | Remove extended attribute |
| **access** | `/api/v8/access/<path>?mode=rw` | GET | Permission check |
| **fallocate** | `/api/v8/fallocate` | POST | Pre-allocate space |
| **lseek** | `/api/v8/lseek/<path>?offset=N&whence=DATA` | GET | Seek to data/hole |

**Example: rename operation**
```http
POST /api/v8/rename
Content-Type: application/json

{
  "old_path": "/tmp/file.txt",
  "new_path": "/home/user/important.txt"
}

Response:
{
  "status": "success",
  "estalecookie": 1234567890123456,
  "new_validator": 1001,
  "invalidate_paths": ["/tmp", "/home/user"]
}
```

**Atomic rename implementation:**
- Server-side operation (not client-side transaction)
- CouchDB batch update: delete old doc, create new doc
- Estalecookie preserved (same inode)
- Parent directories updated with new mtime
- Clients receive update via replication

---

## CouchDB Metadata Replication

### Architecture

**Server-side (Primary):**
- Single CouchDB instance holds authoritative metadata
- All writes go to primary
- Serves as replication source for all clients

**Client-side (Replicas):**
- Local CouchDB instance per client
- Continuous replication from primary
- Read-only from client's perspective
- Metadata operations are local database queries (no network!)

### Metadata Schema

```javascript
{
  // Document ID is the file path
  "_id": "/data/simulation/output.dat",
  
  // CouchDB revision for conflict resolution
  "_rev": "3-abc123def456",
  
  // POSIX attributes
  "mode": 33188,
  "nlink": 1,
  "uid": 1000,
  "gid": 1000,
  "size": 10737418240,
  
  // Timestamps
  "atime": 1234567890,
  "mtime": 1234567890,
  "ctime": 1234567890,
  
  // mosaicfs consistency fields
  "estalecookie": 1570212575206024942,
  "validator": 746338405,
  
  // File type
  "file_type": "f",  // 'f'=file, 'd'=directory, 'l'=symlink
  
  // For symlinks
  "symlink_target": "/actual/path",
  
  // Data plane routing
  "storage": {
    "backend": "nas-01",
    "url": "http://nas-01:8080/data/simulation/output.dat",
    "tier": "hot",
    "checksum": "sha256:abc123...",
    "replicas": [
      {"backend": "nas-01", "priority": 1},
      {"backend": "s3-backup", "priority": 2}
    ]
  },
  
  // Hierarchical navigation
  "parent_path": "/data/simulation",
  "name": "output.dat"
}
```

### Replication Configuration

**Filtered replication (optional):**
```javascript
{
  "source": "http://control.mosaicfs.example.com:5984/mosaicfs_metadata",
  "target": "local_mosaicfs",
  "continuous": true,
  "filter": "mosaicfs/user_paths",
  "query_params": {
    "paths": ["/home/user", "/project/active"]
  }
}
```

**Benefits of filtering:**
- Reduces storage on client (only replicate needed paths)
- Faster initial sync
- Lower bandwidth consumption
- Client can expand filter on-demand

### Change Notifications

**CouchDB changes feed for real-time updates:**
```javascript
const changes = db.changes({
  since: 'now',
  live: true,
  include_docs: true
});

changes.on('change', (change) => {
  // Invalidate kernel cache for changed path
  fuse_invalidate_entry(change.id);
  
  // Update local state
  update_metadata_cache(change.doc);
});
```

### Performance Impact

| Operation | Current mosaicfs (v7) | CouchDB Model (v8) | Speedup |
|-----------|-------------------|-------------------|---------|
| `stat()` uncached | 2,756 µs | ~50 µs | **55x** |
| `stat()` disk cache | 765 µs | ~50 µs | **15x** |
| `readdir()` (1000 files) | 2,756 µs | ~100 µs | **27x** |
| `find /` (8M files) | Minutes | Seconds | **100x+** |
| Offline operation | Manual switch + stale-if-error | Automatic | **Always works** |

---

## Control Plane + Data Plane Architecture

### Separation of Concerns

**Control Plane Responsibilities:**
- Metadata storage and replication (CouchDB)
- RESTful API for filesystem operations
- Write session management and buffering
- Data plane selection and routing
- Consistency and estalecookie management

**Data Plane Responsibilities:**
- Actual file content storage
- Direct client read access (high bandwidth)
- Backend-specific optimizations (NAS, S3, local drives)
- Write access through local mounts or cloud APIs

### Data Plane Types

| Backend Type | Read | Write | Use Case |
|-------------|------|-------|----------|
| **Local drive** | Fastest | Fastest | Hot data on the machine you're using |
| **NAS (NFS/SMB)** | Fast | Fast | Shared storage, accessible from any device |
| **S3/Cloud** | Medium | Medium | Cold storage, archives, backups |

### How Clients Access Data Planes

Each machine has its own view of where data planes are located. The metadata stores a **data plane ID** and a **relative path**, not an absolute URL. Each client maps data plane IDs to local paths in its configuration.

**Example: Reading a file**

The metadata in CouchDB says:
```json
{
  "_id": "/photos/vacation-2024/IMG_001.jpg",
  "storage": {
    "data_plane": "nas-living-room",
    "path": "photos/vacation-2024/IMG_001.jpg"
  }
}
```

On the laptop, the local config (`~/.config/mosaicfs/mounts.toml`) says:
```toml
[data_planes.nas-living-room]
type = "mount"
path = "/Volumes/NAS"  # macOS mount point

[data_planes.nas-office]
type = "mount"
path = "/Volumes/OfficeNAS"

[data_planes.s3-archive]
type = "s3"
bucket = "my-backup-bucket"
region = "us-west-2"

[data_planes.laptop-local]
type = "local"
path = "/Users/me/mosaicfs-local"
```

On the desktop PC, the same data plane has a different path:
```toml
[data_planes.nas-living-room]
type = "mount"
path = "/mnt/nas"  # Linux mount point

[data_planes.desktop-local]
type = "local"
path = "/home/me/mosaicfs-local"
```

When the mosaicfs client on the laptop reads `/photos/vacation-2024/IMG_001.jpg`:
1. It looks up the metadata in local CouchDB
2. Sees `data_plane: "nas-living-room"`, `path: "photos/vacation-2024/IMG_001.jpg"`
3. Looks up `nas-living-room` in `mounts.toml` → `/Volumes/NAS`
4. Reads the file from `/Volumes/NAS/photos/vacation-2024/IMG_001.jpg`

No HTTP, no authentication dance—just a local file read from an already-mounted path.

### Setting Up Your Mounts

Before running mosaicfs, you set up mounts the normal way:

**macOS (SMB to Synology NAS):**
```bash
# Mount once manually, or add to Finder → Connect to Server
mount -t smbfs //user@synology.local/share /Volumes/NAS
```

**Linux (NFS to QNAP):**
```bash
# Add to /etc/fstab for auto-mount
qnap.local:/share  /mnt/nas  nfs  defaults  0  0
```

**S3 (using rclone mount):**
```bash
rclone mount s3:my-backup-bucket /mnt/s3-archive --daemon
```

mosaicfs doesn't manage these mounts—your OS does. mosaicfs just uses them.

### What Happens When a Mount Isn't Available

If you're on your laptop at a coffee shop and try to read a file that lives on your home NAS:

1. mosaicfs tries to read from `/Volumes/NAS/...`
2. The read fails (mount not accessible)
3. mosaicfs returns `EIO` (I/O error) to the application

Possible improvements for the future:
- Fall back to a cloud replica if one exists
- Cache frequently-accessed files locally
- Show "offline" status in the mosaicfs UI

For now, the simple answer is: if you can't reach the storage, you can't read the file. But you can still *see* all your files (metadata is local), and you can still create new files that will sync later.

### Write Path: Where New Files Go

When you create a new file, mosaicfs needs to decide which data plane to put it on. This is configured per-machine:

```toml
# In ~/.config/mosaicfs/config.toml

# New files go here by default
default_data_plane = "laptop-local"

# Tier assignment for auto-migration
[tiers]
hot = "laptop-local"
warm = "nas-living-room"
cold = "s3-archive"
```

**Write flow:**
1. Application creates `/mosaicfs/documents/notes.txt`
2. mosaicfs client picks `laptop-local` as the data plane (per config)
3. File is written to `/Users/me/mosaicfs-local/documents/notes.txt`
4. Metadata is sent to control plane
5. Other clients see the file via CouchDB replication

### Auto-Migration Between Tiers

A background process (running on the control plane or a dedicated machine) can move files between data planes based on rules:

```toml
# In control plane config

[[migration_rules]]
name = "Archive old photos"
condition = "path starts with /photos AND last_access > 90 days"
from_tier = "warm"
to_tier = "cold"

[[migration_rules]]
name = "Keep active projects local"
condition = "path starts with /projects AND last_access < 7 days"
from_tier = "warm"
to_tier = "hot"
```

When a file moves:
1. Data is copied from source data plane to destination
2. Metadata is updated with new `data_plane` and `path`
3. Old copy is deleted
4. Clients see the change via CouchDB replication

From the user's perspective, the file is still at `/mosaicfs/photos/old-vacation.jpg`—it just loads a bit slower now because it's coming from S3 instead of the NAS.

### Data Plane Discovery

The control plane knows about all registered data planes:

```http
GET /api/v8/data-planes

Response:
{
  "data_planes": [
    {
      "id": "nas-living-room",
      "type": "nfs",
      "tier": "warm",
      "capacity_bytes": 4000000000000,
      "used_bytes": 2100000000000
    },
    {
      "id": "s3-archive",
      "type": "s3",
      "tier": "cold",
      "bucket": "my-backup-bucket",
      "region": "us-west-2"
    },
    {
      "id": "laptop-local",
      "type": "local",
      "tier": "hot",
      "owner": "laptop.local"
    },
    {
      "id": "desktop-local",
      "type": "local",
      "tier": "hot",
      "owner": "desktop.local"
    }
  ]
}
```

---

## Implementation Phases

### Phase 1: Core Infrastructure (Months 1-2)

**Deliverables:**
- Rust API server with HTTP/2 support
- CouchDB setup (server + client)
- Basic JSON protocol implementation
- Read-only operations (stat, readdir, read, readlink)

**Success criteria:**
- Clients can query local CouchDB for metadata
- Read operations work end-to-end
- Performance comparable to v7.3 for cached operations
- Metadata replication functional

### Phase 2: Write Support (Months 3-4)

**Deliverables:**
- Write session management (open/write/close)
- File creation with write-once semantics
- Basic metadata operations (mkdir, rmdir, chmod, chown)
- Session cleanup and error handling

**Success criteria:**
- Clients can create new files
- Write sessions work reliably
- Session timeouts and cleanup functional
- estalecookie generation and preservation

### Phase 3: Advanced Operations (Months 5-6)

**Deliverables:**
- Atomic rename implementation
- Hard links (link/unlink)
- Symbolic links
- Extended attributes (setxattr, removexattr)
- Advanced operations (fallocate, lseek, access)

**Success criteria:**
- Rename preserves estalecookie
- All POSIX operations functional
- FUSE integration complete

### Phase 4: Data Plane Integration (Months 7-8)

**Deliverables:**
- Multiple data plane support
- Data plane routing in metadata
- Direct client read access via redirects or embedded URLs
- Write buffering and upload to data planes

**Success criteria:**
- Clients can read from multiple backends
- Data plane selection policies work
- Migration between tiers functional
- S3 and NAS backends operational

### Phase 5: Production Hardening (Months 9-10)

**Deliverables:**
- Comprehensive error handling
- Monitoring and metrics
- Performance optimization
- Migration tools (v7 → v8)
- Documentation and operational guides

**Success criteria:**
- Production-ready deployment
- Migration path validated
- Performance meets or exceeds v7.3
- Operational runbooks complete

---

## Detailed Technical Specifications

### Client Architecture (Rust + CouchDB)

**Components:**
```
┌─────────────────────────────────────┐
│     FUSE Daemon (Rust)              │
│  ┌───────────────────────────────┐  │
│  │  FUSE Callback Handlers       │  │
│  │  - getattr, readdir, read...  │  │
│  └──────────┬────────────────────┘  │
│             │                        │
│  ┌──────────▼────────────────────┐  │
│  │  Metadata Manager             │  │
│  │  - Query local CouchDB        │  │
│  │  - Cache invalidation         │  │
│  └──────────┬────────────────────┘  │
│             │                        │
│  ┌──────────▼────────────────────┐  │
│  │  Data Manager                 │  │
│  │  - HTTP client                │  │
│  │  - Data plane routing         │  │
│  │  - Local data cache           │  │
│  └──────────┬────────────────────┘  │
│             │                        │
│  ┌──────────▼────────────────────┐  │
│  │  Write Session Manager        │  │
│  │  - Session lifecycle          │  │
│  │  - Buffering & upload         │  │
│  └───────────────────────────────┘  │
└─────────────────────────────────────┘
           │         ▲
           │         │
           ▼         │
    ┌─────────────────────┐
    │  Local CouchDB      │
    │  (Metadata replica) │
    └─────────────────────┘
```

**FUSE Integration:**
```rust
impl FuseOperations {
    // Metadata operations (local CouchDB)
    fn getattr(&self, path: &str) -> Result<Stat> {
        let doc = self.local_couch.get(path).await?;
        Ok(parse_stat(doc))
    }
    
    // Data operations (data plane)
    fn read(&self, path: &str, offset: u64, len: usize) -> Result<Vec<u8>> {
        // Get data URL from metadata
        let metadata = self.local_couch.get(path).await?;
        let data_url = metadata["storage"]["url"].as_str()?;
        
        // Fetch from data plane
        let response = self.http_client
            .get(data_url)
            .header("Range", format!("bytes={}-{}", offset, offset + len - 1))
            .send()
            .await?;
        
        Ok(response.bytes().await?.to_vec())
    }
    
    // Write operations (session-based)
    fn create(&self, path: &str, mode: u32) -> Result<FileHandle> {
        let session = self.control_plane
            .open(path, mode)
            .await?;
        
        let fh = self.allocate_file_handle();
        self.write_sessions.insert(fh, session);
        Ok(fh)
    }
    
    fn write(&self, fh: FileHandle, data: &[u8]) -> Result<usize> {
        let session = self.write_sessions.get_mut(&fh)?;
        self.control_plane
            .upload(session.id, data)
            .await?;
        Ok(data.len())
    }
    
    fn release(&self, fh: FileHandle) -> Result<()> {
        if let Some(session) = self.write_sessions.remove(&fh) {
            self.control_plane
                .close(session.id)
                .await?;
            self.invalidate_cache(&session.path);
        }
        Ok(())
    }
}
```

### Server Architecture (Rust + CouchDB)

**Components:**
```
┌─────────────────────────────────────┐
│   Rust API Server (Control Plane)  │
│  ┌───────────────────────────────┐  │
│  │  HTTP/2 API Layer             │  │
│  │  (Axum/Actix-web)             │  │
│  └──────────┬────────────────────┘  │
│             │                        │
│  ┌──────────▼────────────────────┐  │
│  │  Metadata Service             │  │
│  │  - CouchDB client             │  │
│  │  - estalecookie generation    │  │
│  │  - validator management       │  │
│  └──────────┬────────────────────┘  │
│             │                        │
│  ┌──────────▼────────────────────┐  │
│  │  Data Plane Router            │  │
│  │  - Backend selection          │  │
│  │  - Policy engine              │  │
│  │  - Tier management            │  │
│  └──────────┬────────────────────┘  │
│             │                        │
│  ┌──────────▼────────────────────┐  │
│  │  Write Session Manager        │  │
│  │  - Session lifecycle          │  │
│  │  - Buffering                  │  │
│  │  - Upload orchestration       │  │
│  └───────────────────────────────┘  │
└─────────────────────────────────────┘
           │         ▲
           │         │
           ▼         │
    ┌─────────────────────┐
    │  Primary CouchDB    │
    │  (Metadata master)  │
    └─────────────────────┘
```

**API Handler Example:**
```rust
// Axum route handler
async fn handle_create(
    Json(req): Json<CreateRequest>,
    State(state): State<AppState>,
) -> Result<Json<CreateResponse>> {
    // Validate request
    if state.couch.exists(&req.path).await? {
        return Err(Error::AlreadyExists);
    }
    
    // Create write session
    let session_id = Uuid::new_v4();
    let estalecookie = generate_estalecookie();
    
    state.couch.put(json!({
        "_id": format!("session:{}", session_id),
        "path": req.path,
        "mode": req.mode,
        "estalecookie": estalecookie,
        "status": "open",
        "created_at": Utc::now(),
        "expires_at": Utc::now() + Duration::hours(1)
    })).await?;
    
    Ok(Json(CreateResponse {
        session_id,
        upload_url: format!("/api/v8/upload/{}", session_id),
        expires_at: (Utc::now() + Duration::hours(1)).timestamp()
    }))
}
```

### Database Design

**CouchDB Views for Queries:**
```javascript
// Design document for filesystem queries
{
  "_id": "_design/filesystem",
  "views": {
    // Directory listing
    "by_parent": {
      "map": "function(doc) {
        if (doc.parent_path) {
          emit(doc.parent_path, {
            name: doc.name,
            type: doc.file_type,
            size: doc.size,
            mtime: doc.mtime
          });
        }
      }"
    },
    
    // Find by file type
    "by_type": {
      "map": "function(doc) {
        if (doc.file_type) {
          emit(doc.file_type, doc._id);
        }
      }"
    },
    
    // Storage tier distribution
    "by_tier": {
      "map": "function(doc) {
        if (doc.storage && doc.storage.tier) {
          emit(doc.storage.tier, doc.size);
        }
      }",
      "reduce": "_sum"
    }
  }
}
```

**Indexes for Performance:**
```javascript
// Mango index for path searches
{
  "index": {
    "fields": ["parent_path", "name"]
  },
  "name": "parent-name-index",
  "type": "json"
}

// Index for estalecookie lookups
{
  "index": {
    "fields": ["estalecookie"]
  },
  "name": "estalecookie-index",
  "type": "json"
}
```

---

## Migration Strategy

v8 is a clean break from v7.3. There is no dual-protocol transition period. All clients and servers must be upgraded simultaneously. The v7.3 protocol code is deleted from the codebase.

## Testing Strategy

### Unit Tests

**Metadata operations:**
```rust
#[tokio::test]
async fn test_stat_operation() {
    let couch = setup_test_couch().await;
    
    // Insert test document
    couch.put(json!({
        "_id": "/test/file.txt",
        "mode": 33188,
        "size": 1024,
        "estalecookie": 123456789
    })).await.unwrap();
    
    // Query
    let metadata = couch.get("/test/file.txt").await.unwrap();
    
    assert_eq!(metadata["mode"].as_u64(), Some(33188));
    assert_eq!(metadata["size"].as_u64(), Some(1024));
}
```

**Write sessions:**
```rust
#[tokio::test]
async fn test_write_session() {
    let server = TestServer::new().await;
    
    // Open
    let session = server.open("/test/newfile.txt", 0644).await.unwrap();
    
    // Write
    server.upload(&session.id, b"Hello World").await.unwrap();
    
    // Close
    let result = server.close(&session.id).await.unwrap();
    
    assert_eq!(result.size, 11);
    assert!(result.estalecookie > 0);
}
```

### Integration Tests

**End-to-end workflow:**
```rust
#[tokio::test]
async fn test_file_lifecycle() {
    let server = setup_test_server().await;
    let client = setup_test_client(&server).await;
    
    // Create file
    let fh = client.create("/test/file.txt", 0644).await.unwrap();
    client.write(fh, b"test data").await.unwrap();
    client.release(fh).await.unwrap();
    
    // Verify metadata replicated
    tokio::time::sleep(Duration::from_secs(1)).await;
    let stat = client.stat("/test/file.txt").await.unwrap();
    assert_eq!(stat.size, 9);
    
    // Read back
    let data = client.read("/test/file.txt", 0, 9).await.unwrap();
    assert_eq!(data, b"test data");
}
```

### Performance Tests

**Benchmarks:**
```rust
#[bench]
fn bench_local_stat(b: &mut Bencher) {
    let couch = setup_bench_couch();
    b.iter(|| {
        couch.get("/bench/file.txt").await
    });
}

#[bench]
fn bench_readdir_1000(b: &mut Bencher) {
    let couch = setup_bench_couch_with_entries(1000);
    b.iter(|| {
        couch.query_view("filesystem/by_parent", "/bench").await
    });
}
```

**Load tests:**
- Concurrent client connections: 1000+
- Metadata operations/sec: 10,000+
- Data throughput: Match or exceed v7.3
- Replication lag: < 1 second for metadata updates

---

## Operational Considerations

### Monitoring

**Key metrics:**
- CouchDB replication lag (per client)
- Write session success/failure rates
- Data plane availability and latency
- API endpoint response times
- Cache hit rates (metadata and data)
- Disk usage (client CouchDB, control plane buffer)

**Prometheus metrics:**
```rust
// API metrics
http_requests_total{endpoint="/api/v8/stat", status="200"}
http_request_duration_seconds{endpoint="/api/v8/stat"}

// CouchDB metrics
couch_replication_lag_seconds{client="client-01"}
couch_docs_total{database="mosaicfs_metadata"}

// Write session metrics
write_sessions_active
write_sessions_completed_total
write_sessions_failed_total{reason="timeout"}

// Data plane metrics
data_plane_requests_total{backend="nas-01", operation="read"}
data_plane_latency_seconds{backend="nas-01"}
```

### Backup and Recovery

**CouchDB backup:**
```bash
# Continuous backup via replication
curl -X POST http://localhost:5984/_replicate \
  -H "Content-Type: application/json" \
  -d '{
    "source": "mosaicfs_metadata",
    "target": "http://backup-server:5984/mosaicfs_metadata_backup",
    "continuous": true
  }'
```

**Point-in-time recovery:**
- CouchDB snapshots (daily)
- Transaction log replay
- Data plane content checksums (verify integrity)

### Security

**Authentication:**
- API tokens for clients
- TLS/mTLS for client-server communication
- CouchDB authentication for replication
- Data plane access controls (per-backend)

**Authorization:**
- Path-based permissions in metadata
- POSIX uid/gid enforcement
- Data plane signed URLs (time-limited)

---

## Success Criteria

### Functional Requirements

- ✅ All read operations functional
- ✅ Write operations with write-once semantics
- ✅ Metadata replication to clients
- ✅ Multiple data plane backends
- ✅ Direct client access to data planes
- ✅ Atomic rename preserves estalecookie
- ✅ Offline operation for metadata queries

---

## Open Questions and Future Work

### Open Questions

1. **Large file writes:** How to handle multi-GB uploads (like video files)?
    - Options: Chunked uploads, resumable sessions, direct writes to data plane
    - For now: Write directly to the local data plane, let migration move it later

2. **Conflict resolution:** What happens if two machines edit the same file?
    - Current: Last writer wins (simple)
    - Acceptable for home use where conflicts are rare
    - Future: Show conflicts in a special folder for manual resolution

3. **Mount reliability:** What if NFS/SMB mounts drop unexpectedly?
    - Detect mount failures and report clearly to the user
    - Don't hang indefinitely on unresponsive mounts
    - Consider auto-remount with exponential backoff

4. **Initial sync:** How long does first-time metadata replication take?
    - For a home setup with ~100,000 files: probably seconds to minutes
    - Consider progress indicator during initial sync

### Resolved Questions

1. ~~**Data plane authentication:** How to secure direct client access?~~
   - **Resolved:** Users configure NFS/CIFS mounts through their OS using standard authentication (passwords, Kerberos, etc.). mosaicfs uses these existing mounts. For S3, standard AWS credential configuration applies.

### Future Enhancements

**Phase 6: Better Offline Experience**
- Local caching of frequently-accessed files from remote data planes
- Prefetch files based on access patterns
- Queue writes when offline, sync when network returns
- Visual indicator of file availability (local vs. remote vs. offline)

**Phase 7: Smarter Migration**
- Learn access patterns to predict which files to keep local
- Automatic deduplication across data planes
- Compression for cold storage tier
- Thumbnail/preview generation for media files

**Phase 8: Easier Setup**
- GUI for configuring data planes and mounts
- Auto-discovery of NAS devices on the local network
- One-click S3 bucket setup
- Mobile app for browsing files (read-only)

---

## Appendix

### Technology Stack

**Server:**
- Language: Rust (async/await with tokio)
- Web framework: Axum or Actix-web
- Database: CouchDB 3.x
- HTTP client: reqwest
- Protocol: HTTP/2 (h2)

**Client:**
- Language: Rust
- FUSE: fuse-rs or polyfuse
- Database: CouchDB 3.x or PouchDB (embedded)
- HTTP client: reqwest with connection pooling
- Cache: Custom disk cache layer

**Data Plane:**
- Local drives: Direct filesystem access
- NAS: User-configured NFS/SMB mounts (no HTTP wrapper needed)
- S3: AWS SDK or rclone mount

### References

- CouchDB replication protocol documentation
- HTTP/2 RFC 7540
- FUSE lowlevel API documentation
- POSIX filesystem semantics
