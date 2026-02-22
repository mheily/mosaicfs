import { useState, useEffect, useRef } from 'react';
import { getDB } from '@/lib/pouchdb';

export function useLiveQuery<T = unknown>(
  selector: PouchDB.Find.Selector,
  fields?: string[],
) {
  const [data, setData] = useState<T[]>([]);
  const [loading, setLoading] = useState(true);
  const selectorRef = useRef(JSON.stringify(selector));
  const fieldsRef = useRef(JSON.stringify(fields));

  useEffect(() => {
    selectorRef.current = JSON.stringify(selector);
    fieldsRef.current = JSON.stringify(fields);
  });

  useEffect(() => {
    const db = getDB();
    let cancelled = false;

    async function runQuery() {
      try {
        const req: PouchDB.Find.FindRequest<object> = {
          selector,
          limit: 10000,
        };
        if (fields) req.fields = fields;
        const result = await db.find(req);
        if (!cancelled) {
          setData(result.docs as T[]);
          setLoading(false);
        }
      } catch {
        if (!cancelled) setLoading(false);
      }
    }

    runQuery();

    const changes = db
      .changes({ since: 'now', live: true })
      .on('change', () => {
        runQuery();
      });

    return () => {
      cancelled = true;
      changes.cancel();
    };
  }, [selectorRef.current, fieldsRef.current]);

  return { data, loading };
}
