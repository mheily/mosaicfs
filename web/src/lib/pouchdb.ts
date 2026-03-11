import PouchDB from 'pouchdb';
import PouchDBFind from 'pouchdb-find';
import { getAuthToken, getBaseUrl } from '@/lib/api';

PouchDB.plugin(PouchDBFind);

let localDB: PouchDB.Database | null = null;
let replication: PouchDB.Replication.Replication<object> | null = null;

export function getDB(): PouchDB.Database {
  if (!localDB) {
    localDB = new PouchDB('mosaicfs');
  }
  return localDB;
}

// pouchdb-find requires explicit indexes created via db.createIndex() to work
// reliably. Design documents synced from CouchDB are not automatically compiled
// into usable local indexes. Create the indexes we need before any queries run.
async function ensureIndexes(db: PouchDB.Database) {
  await db.createIndex({ index: { fields: ['type'] } });
  await db.createIndex({ index: { fields: ['type', 'status'] } });
}

export function startSync(remoteUrl: string) {
  const db = getDB();

  ensureIndexes(db).catch(() => {/* non-fatal: queries fall back to full scan */});

  // PouchDB uses the URL scheme to select its adapter: only http/https triggers
  // the HTTP adapter. A path like "/db/mosaicfs" would fall back to IndexedDB,
  // creating a local database instead of connecting to the remote. Resolve to
  // an absolute URL first.
  const base = getBaseUrl() || window.location.origin;
  const absoluteUrl = remoteUrl.startsWith('http')
    ? remoteUrl
    : new URL(remoteUrl, base).href;
  const remote = new PouchDB(absoluteUrl, {
    skip_setup: true,
    fetch: (url, opts) => {
      const token = getAuthToken();
      if (token) {
        const headers = new Headers((opts as RequestInit).headers);
        headers.set('Authorization', `Bearer ${token}`);
        (opts as RequestInit).headers = headers;
      }
      return PouchDB.fetch(url, opts);
    },
  });

  replication = db.replicate.from(remote, {
    live: true,
    retry: true,
  });

  return replication;
}

export async function destroyDB() {
  if (replication) {
    replication.cancel();
    replication = null;
  }
  if (localDB) {
    await localDB.destroy();
    localDB = null;
  }
}
