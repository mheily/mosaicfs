import { useState, useEffect, useCallback, useRef } from 'react';
import { useSearchParams } from 'react-router-dom';
import { api } from '@/lib/api';
import { useDebounce } from '@/hooks/useDebounce';
import { formatBytes } from '@/lib/format';
import { Search, Loader2, X } from 'lucide-react';
import { FileDetailDrawer } from '@/components/FileDetailDrawer';

interface SearchResult {
  name: string;
  path: string;
  node: string;
  size: number;
  is_dir?: boolean;
  mtime?: string;
}

interface LabelDef {
  name: string;
}

const PAGE_SIZE = 50;

export default function SearchPage() {
  const [searchParams, setSearchParams] = useSearchParams();
  const initialQ = searchParams.get('q') || '';

  const [query, setQuery] = useState(initialQ);
  const debouncedQuery = useDebounce(query, 300);

  const [results, setResults] = useState<SearchResult[]>([]);
  const [loading, setLoading] = useState(false);
  const [hasMore, setHasMore] = useState(false);
  const [offset, setOffset] = useState(0);

  const [labels, setLabels] = useState<LabelDef[]>([]);
  const [activeLabels, setActiveLabels] = useState<Set<string>>(new Set());
  const [selectedFile, setSelectedFile] = useState<SearchResult | null>(null);

  const loaderRef = useRef<HTMLDivElement>(null);

  // Load available labels
  useEffect(() => {
    api<LabelDef[]>('/api/labels')
      .then(setLabels)
      .catch(() => setLabels([]));
  }, []);

  const doSearch = useCallback(
    async (q: string, newOffset: number, append: boolean) => {
      if (!q.trim()) {
        if (!append) setResults([]);
        return;
      }
      setLoading(true);
      try {
        let url = `/api/search?q=${encodeURIComponent(q)}&limit=${PAGE_SIZE}&offset=${newOffset}`;
        if (activeLabels.size > 0) {
          url += `&labels=${encodeURIComponent([...activeLabels].join(','))}`;
        }
        const data = await api<SearchResult[]>(url);
        if (append) {
          setResults((prev) => [...prev, ...data]);
        } else {
          setResults(data);
        }
        setHasMore(data.length === PAGE_SIZE);
      } catch {
        if (!append) setResults([]);
      } finally {
        setLoading(false);
      }
    },
    [activeLabels],
  );

  // Search on debounced query change
  useEffect(() => {
    setOffset(0);
    doSearch(debouncedQuery, 0, false);
    if (debouncedQuery) {
      setSearchParams({ q: debouncedQuery }, { replace: true });
    }
  }, [debouncedQuery, doSearch, setSearchParams]);

  // Infinite scroll
  useEffect(() => {
    const el = loaderRef.current;
    if (!el) return;

    const observer = new IntersectionObserver(
      (entries) => {
        if (entries[0].isIntersecting && hasMore && !loading) {
          const newOffset = offset + PAGE_SIZE;
          setOffset(newOffset);
          doSearch(debouncedQuery, newOffset, true);
        }
      },
      { threshold: 0.1 },
    );

    observer.observe(el);
    return () => observer.disconnect();
  }, [hasMore, loading, offset, debouncedQuery, doSearch]);

  function toggleLabel(label: string) {
    setActiveLabels((prev) => {
      const next = new Set(prev);
      if (next.has(label)) next.delete(label);
      else next.add(label);
      return next;
    });
  }

  return (
    <div className="p-6">
      <h1 className="mb-4 text-2xl font-bold">Search</h1>

      {/* Search input */}
      <div className="relative mb-4">
        <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
        <input
          type="text"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Search files by name, path, or content..."
          className="flex h-10 w-full rounded-md border border-input bg-transparent pl-9 pr-3 py-2 text-sm shadow-sm placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          autoFocus
        />
      </div>

      {/* Label filter chips */}
      {labels.length > 0 && (
        <div className="mb-4 flex flex-wrap gap-2">
          {labels.map((l) => (
            <button
              key={l.name}
              onClick={() => toggleLabel(l.name)}
              className={`inline-flex items-center gap-1 rounded-full border px-3 py-1 text-xs font-medium transition-colors ${
                activeLabels.has(l.name)
                  ? 'border-primary bg-primary text-primary-foreground'
                  : 'border-border hover:bg-accent'
              }`}
            >
              {l.name}
              {activeLabels.has(l.name) && <X className="h-3 w-3" />}
            </button>
          ))}
        </div>
      )}

      {/* Results table */}
      {results.length > 0 ? (
        <table className="w-full text-sm">
          <thead>
            <tr className="border-b text-left text-muted-foreground">
              <th className="pb-2 font-medium">Name</th>
              <th className="pb-2 font-medium">Path</th>
              <th className="pb-2 font-medium">Node</th>
              <th className="pb-2 font-medium">Size</th>
            </tr>
          </thead>
          <tbody>
            {results.map((r, i) => (
              <tr
                key={`${r.path}-${i}`}
                className="cursor-pointer border-b hover:bg-accent"
                onClick={() => setSelectedFile(r)}
              >
                <td className="py-2 font-medium">{r.name}</td>
                <td className="py-2 text-muted-foreground">{r.path}</td>
                <td className="py-2">{r.node}</td>
                <td className="py-2">{formatBytes(r.size)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      ) : (
        !loading &&
        debouncedQuery && (
          <p className="py-8 text-center text-muted-foreground">No results found</p>
        )
      )}

      {/* Infinite scroll trigger */}
      <div ref={loaderRef} className="flex justify-center py-4">
        {loading && <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />}
      </div>

      {selectedFile && (
        <FileDetailDrawer
          file={selectedFile}
          onClose={() => setSelectedFile(null)}
        />
      )}
    </div>
  );
}
