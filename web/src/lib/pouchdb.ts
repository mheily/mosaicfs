import PouchDB from 'pouchdb';
import PouchDBFind from 'pouchdb-find';

PouchDB.plugin(PouchDBFind);

let localDB: PouchDB.Database | null = null;
let replication: PouchDB.Replication.Replication<object> | null = null;

export function getDB(): PouchDB.Database {
  if (!localDB) {
    localDB = new PouchDB('mosaicfs');
  }
  return localDB;
}

export function startSync(remoteUrl: string) {
  const db = getDB();
  const remote = new PouchDB(remoteUrl, { skip_setup: true });

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
