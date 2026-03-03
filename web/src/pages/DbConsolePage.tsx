import { useState } from 'react';
import { getDB } from '@/lib/pouchdb';
import { Play, RotateCcw } from 'lucide-react';

const PRESETS = [
  { label: 'All documents', selector: '{ "selector": {}, "limit": 50 }' },
  { label: 'Files', selector: '{ "selector": { "type": "file" }, "limit": 50 }' },
  { label: 'Label assignments', selector: '{ "selector": { "type": "label_assignment" } }' },
  { label: 'Virtual directories', selector: '{ "selector": { "type": "virtual_directory" } }' },
  { label: 'Nodes', selector: '{ "selector": { "type": "node" } }' },
  { label: 'Label rules', selector: '{ "selector": { "type": "label_rule" } }' },
];

export default function DbConsolePage() {
  const [input, setInput] = useState(PRESETS[0].selector);
  const [output, setOutput] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const [rowCount, setRowCount] = useState<number | null>(null);

  async function runQuery() {
    setRunning(true);
    setError(null);
    setOutput(null);
    setRowCount(null);
    try {
      const query = JSON.parse(input);
      const db = getDB();
      const result = await db.find(query);
      setOutput(JSON.stringify(result.docs, null, 2));
      setRowCount(result.docs.length);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setRunning(false);
    }
  }

  function handleKeyDown(e: React.KeyboardEvent<HTMLTextAreaElement>) {
    if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
      e.preventDefault();
      runQuery();
    }
  }

  return (
    <div className="flex h-full flex-col gap-4 p-6">
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-bold">Database Console</h1>
        <span className="text-xs text-muted-foreground">Local PouchDB — mosaicfs</span>
      </div>

      {/* Presets */}
      <div className="flex flex-wrap gap-2">
        {PRESETS.map((p) => (
          <button
            key={p.label}
            onClick={() => setInput(p.selector)}
            className="rounded-full border px-3 py-1 text-xs hover:bg-accent"
          >
            {p.label}
          </button>
        ))}
      </div>

      {/* Query input */}
      <div className="space-y-1">
        <div className="flex items-center justify-between">
          <label className="text-xs font-medium text-muted-foreground">
            PouchDB <code className="font-mono">db.find()</code> query (JSON)
          </label>
          <div className="flex items-center gap-2">
            <button
              onClick={() => { setInput(PRESETS[0].selector); setOutput(null); setError(null); setRowCount(null); }}
              className="inline-flex items-center gap-1 rounded px-2 py-1 text-xs text-muted-foreground hover:bg-accent"
            >
              <RotateCcw className="h-3 w-3" />
              Reset
            </button>
            <button
              onClick={runQuery}
              disabled={running}
              className="inline-flex items-center gap-1.5 rounded-md bg-primary px-3 py-1.5 text-xs font-medium text-primary-foreground hover:bg-primary/90 disabled:opacity-50"
            >
              <Play className="h-3 w-3" />
              {running ? 'Running…' : 'Run'}
              <span className="text-primary-foreground/60">⌘↵</span>
            </button>
          </div>
        </div>
        <textarea
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={handleKeyDown}
          spellCheck={false}
          rows={6}
          className="w-full rounded-md border border-input bg-muted/40 px-3 py-2 font-mono text-sm focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        />
      </div>

      {/* Output */}
      {error && (
        <div className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 font-mono text-xs text-destructive">
          {error}
        </div>
      )}

      {output !== null && (
        <div className="flex min-h-0 flex-1 flex-col space-y-1">
          <div className="flex items-center justify-between">
            <span className="text-xs font-medium text-muted-foreground">Results</span>
            {rowCount !== null && (
              <span className="text-xs text-muted-foreground">{rowCount} document{rowCount !== 1 ? 's' : ''}</span>
            )}
          </div>
          <pre className="min-h-0 flex-1 overflow-auto rounded-md border bg-muted/40 px-3 py-2 font-mono text-xs">
            {output}
          </pre>
        </div>
      )}
    </div>
  );
}
