import { useState, useEffect } from 'react';
import { getDB } from '@/lib/pouchdb';

export function useLiveDoc<T = unknown>(docId: string | null) {
  const [doc, setDoc] = useState<T | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    if (!docId) {
      setDoc(null);
      setLoading(false);
      return;
    }

    const db = getDB();
    let cancelled = false;

    async function fetch() {
      try {
        const result = await db.get(docId!);
        if (!cancelled) {
          setDoc(result as T);
          setLoading(false);
        }
      } catch {
        if (!cancelled) {
          setDoc(null);
          setLoading(false);
        }
      }
    }

    fetch();

    const changes = db
      .changes({
        since: 'now',
        live: true,
        doc_ids: [docId],
      })
      .on('change', () => fetch());

    return () => {
      cancelled = true;
      changes.cancel();
    };
  }, [docId]);

  return { doc, loading };
}
