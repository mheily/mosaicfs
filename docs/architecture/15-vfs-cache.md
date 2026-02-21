<\!-- MosaicFS Architecture · ../architecture.md -->

## VFS File Cache

The cache is part of the common `mosaicfs-vfs` layer, shared across all OS-specific backends. It has two operating modes that share a common on-disk layout and SQLite index. **Full-file mode** is used for small files — the entire file is downloaded in one request and cached atomically. **Block mode** is used for large files, primarily home media (videos, audio, large archives) — individual regions of the file are fetched on demand as read requests arrive, allowing a 40 GB video to be played without downloading it in full. The threshold between modes is configurable (default 50 MB).

### On-Disk Structure

```
/var/lib/MosaicFS/cache/
  a3/f72b1c...          # sparse data file, keyed by file_uuid
  tmp/                  # in-progress full-file downloads
  index.db              # SQLite: all cache metadata
```

Each cache entry is keyed by the file's UUID (from the file document `_id`). The first two characters of the UUID are used as a shard prefix. All data files are sparse files: the OS allocates disk space only for regions that have been written, and unwritten regions read as zeros without consuming disk space. Sparse file support is available on ext4, APFS, NTFS, and ZFS — all target platforms for MosaicFS agents.

### SQLite Index Schema

The `index.db` SQLite database is the single source of truth for all cache metadata. It is updated transactionally so a crash between a write and a metadata update cannot produce an inconsistent state.

```sql
CREATE TABLE cache_entries (
    cache_key       TEXT PRIMARY KEY,   -- file_uuid
    file_id         TEXT NOT NULL,     -- full file document _id
    file_size       INTEGER NOT NULL,
    mtime           TEXT NOT NULL,      -- ISO 8601, for invalidation
    size_on_record  INTEGER NOT NULL,   -- file size at cache time, for invalidation
    block_size      INTEGER,            -- NULL for full-file entries
    block_map       BLOB,               -- NULL for full-file entries; see Block Map below
    cached_bytes    INTEGER NOT NULL,   -- bytes present on disk, for eviction accounting
    last_access     TEXT NOT NULL,      -- ISO 8601, for LRU eviction
    source          TEXT NOT NULL DEFAULT 'remote'
                                        -- 'remote' | 'plugin:{plugin_name}'
);
```

The `source` column is diagnostic — it identifies whether a cache entry was populated by a Tier 4 remote download or by a Tier 5 plugin materialize. It has no effect on cache logic; it is reported in `agent_status` cache statistics and useful for debugging storage backend behavior.

### Block Map

For block-mode entries, the `block_map` column stores a sorted list of non-overlapping, non-adjacent intervals of present blocks. Each interval is a `[start_block, end_block)` half-open range in block-index units.

```
Example: 40 GB file at 1 MB block size
  User watched 0:00–5:00, then seeked to 45:00 and watched 45:00–50:00

  block_map = [(0, 300), (2700, 3000)]
              first 300 blocks present, blocks 2700–2999 present
              2 intervals, ~40 bytes serialized
```

This representation is efficient for the home media use case. A typical viewing session with a few seeks produces 3–15 intervals regardless of file size. The intervals are stored as a compact binary blob (pairs of little-endian u64 values) in the `block_map` column.

**Operations on the block map:**

*Is block N present?* Binary search the interval list for an interval containing N. O(log k) where k is the number of intervals — in practice k < 20.

*Which blocks are missing in range [A, B)?* Take the requested range, subtract all overlapping intervals, and return the remaining gaps as a list of sub-ranges. Each gap becomes one HTTP range request to the remote agent.

*Mark blocks [A, B) as present:* Insert the interval, then merge with any adjacent or overlapping intervals to maintain the sorted, non-overlapping invariant. O(k) worst case, O(1) for the common case of appending to the last interval (sequential playback).

**Fragmentation guard.** If pathological access patterns cause the interval count to exceed 1,000, the block map is promoted to a full-file download: the agent fetches all missing ranges, coalesces the intervals into a single `[(0, total_blocks)]`, and updates the cache entry. This caps the block map blob size at ~16 KB and prevents unbounded growth. The threshold is checked after each block map update.

A roaring bitmap is a suitable future upgrade if access patterns turn out to be more fragmented than the home media use case produces — for example, random-access reads against large database files. For now the interval list is simpler, debuggable, and amply fast.

### Full-File Mode Request Flow

```
read(file_id, offset, length) — file below threshold

  cache hit:
    read bytes from sparse data file at offset, return

  cache miss:
    fetch full file via GET /api/agent/transfer/{file_id}  (HTTP 200)
    stream to cache/tmp/{cache_key}
    verify Digest trailer
    move atomically to cache/{shard}/{cache_key}
    insert row into index.db (block_map = NULL)
    return requested bytes
```

### Block Mode Request Flow

```
read(file_id, offset, length) — file above threshold

  compute block range:
    first_block = offset / block_size
    last_block  = (offset + length - 1) / block_size

  check block map for blocks [first_block, last_block]:
    all present → read bytes from sparse data file, return immediately

    some missing → compute list of missing sub-ranges
                   coalesce adjacent missing blocks into contiguous spans
                   for each span:
                     fetch via GET /api/agent/transfer/{file_id}
                                  Range: bytes={start}-{end}  (HTTP 206)
                     write fetched bytes into sparse data file at correct offset
                   update block map in index.db (single transaction)
                   read assembled bytes from sparse data file, return
```

Missing blocks are coalesced before fetching: if blocks 5, 6, and 7 are all absent, one `Range: bytes=5242880-8388607` request is issued rather than three separate requests. This minimises round trips during the common case of sequential playback with a cold cache.

### Transfer Integrity

Full-file responses (HTTP 200) include a `Digest` trailer (RFC 9530, `sha-256`) computed as the bytes stream. The receiving agent accumulates the hash as it writes to `cache/tmp/`, reads the trailer after the stream closes, and verifies before moving the file to its final location. A mismatch causes the download to be discarded and retried.

```
Trailer: Digest
[response body stream]
Digest: sha-256=:base64encodedchecksum:
```

Range responses (HTTP 206) used by block mode do not carry a `Digest` trailer. TLS provides in-transit integrity for partial fetches. This is appropriate for the threat model: MosaicFS agents are trusted nodes on a home network, and the primary integrity risk is bugs in streaming code (truncation, wrong offset) rather than a malicious intermediary. A corrupted block produces visible video artefacts rather than silent data loss, which is detectable and recoverable by re-fetching the affected blocks.

### Eviction

LRU eviction operates on whole cache entries, not individual blocks. An entry's `cached_bytes` column tracks how much disk space it occupies (for block-mode entries, the number of present blocks × block size). The evictor selects entries by ascending `last_access`, accumulating `cached_bytes` until the size cap or free space constraint is satisfied.

Configurable thresholds: total cache size cap (default 10 GB) and minimum free space on the cache volume (default 1 GB). Eviction runs as a background task after each new cache write.

Partially-cached block-mode entries are evictable — the entire sparse file and its `index.db` row are removed together. There is no partial eviction of individual blocks within an entry. This keeps the eviction logic simple at the cost of occasionally re-fetching blocks that were recently accessed but belonged to an entry that was evicted as a whole.

### Invalidation

The PouchDB live changes feed triggers invalidation when a file document's `mtime` or `size` changes. On each cache lookup, the stored `mtime` and `size_on_record` are compared to the current file document. A mismatch causes the entry to be treated as stale: the sparse data file and `index.db` row are deleted, and the file is re-fetched on the next read.

On agent startup, the cache index is reconciled against the local CouchDB replica. Entries with no matching file document are removed. Entries whose `mtime` or `size` no longer matches the document are removed.

### Download Deduplication

Concurrent reads for the same uncached block range share a single in-flight fetch via a `Shared` future keyed by `(file_id, block_range)`. All waiters receive the result when the fetch completes. This prevents redundant range requests when multiple processes read the same region of a file simultaneously — common when a video player issues read-ahead requests in parallel.

---

