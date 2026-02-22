import { useState, useEffect } from 'react';
import { useLiveQuery } from '@/hooks/useLiveQuery';
import { api } from '@/lib/api';
import { formatBytes, percentColor, percentBarColor } from '@/lib/format';
import { HardDrive } from 'lucide-react';
import {
  LineChart,
  Line,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
} from 'recharts';

interface NodeDoc {
  _id: string;
  type: string;
  name: string;
  node_id?: string;
  capacity?: number;
  used?: number;
  available?: number;
}

interface HistoryPoint {
  time: string;
  used: number;
  capacity: number;
}

const DATE_RANGES = [
  { label: '7 days', days: 7 },
  { label: '30 days', days: 30 },
  { label: '90 days', days: 90 },
] as const;

export default function StoragePage() {
  const { data: nodes, loading } = useLiveQuery<NodeDoc>({ type: 'node' });
  const [selectedNode, setSelectedNode] = useState<string | null>(null);
  const [days, setDays] = useState(30);
  const [history, setHistory] = useState<HistoryPoint[]>([]);
  const [historyLoading, setHistoryLoading] = useState(false);

  // Auto-select first node
  useEffect(() => {
    if (!selectedNode && nodes.length > 0) {
      setSelectedNode(nodes[0].node_id || nodes[0]._id);
    }
  }, [nodes, selectedNode]);

  useEffect(() => {
    if (!selectedNode) return;
    setHistoryLoading(true);
    api<HistoryPoint[]>(`/api/storage/${selectedNode}/history?days=${days}`)
      .then(setHistory)
      .catch(() => setHistory([]))
      .finally(() => setHistoryLoading(false));
  }, [selectedNode, days]);

  return (
    <div className="p-6">
      <h1 className="mb-6 text-2xl font-bold">Storage</h1>

      {/* Utilization Table */}
      <div className="mb-8">
        <h2 className="mb-3 text-lg font-semibold">Node Utilization</h2>
        {loading ? (
          <p className="text-sm text-muted-foreground">Loading...</p>
        ) : (
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b text-left text-muted-foreground">
                <th className="pb-2 font-medium">Node</th>
                <th className="pb-2 font-medium">Capacity</th>
                <th className="pb-2 font-medium">Used</th>
                <th className="pb-2 font-medium">Available</th>
                <th className="pb-2 font-medium w-48">Usage</th>
              </tr>
            </thead>
            <tbody>
              {nodes.map((node) => {
                const capacity = node.capacity || 0;
                const used = node.used || 0;
                const available = node.available || capacity - used;
                const pct = capacity > 0 ? Math.round((used / capacity) * 100) : 0;

                return (
                  <tr key={node._id} className="border-b">
                    <td className="py-2 font-medium">
                      <div className="flex items-center gap-2">
                        <HardDrive className="h-4 w-4 text-muted-foreground" />
                        {node.name}
                      </div>
                    </td>
                    <td className="py-2">{formatBytes(capacity)}</td>
                    <td className="py-2">{formatBytes(used)}</td>
                    <td className="py-2">{formatBytes(available)}</td>
                    <td className="py-2">
                      <div className="flex items-center gap-2">
                        <div className="h-2 flex-1 rounded-full bg-muted">
                          <div
                            className={`h-2 rounded-full ${percentBarColor(pct)}`}
                            style={{ width: `${pct}%` }}
                          />
                        </div>
                        <span className={`text-xs font-medium ${percentColor(pct)}`}>
                          {pct}%
                        </span>
                      </div>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </div>

      {/* Trend Chart */}
      <div>
        <h2 className="mb-3 text-lg font-semibold">Storage Trend</h2>

        <div className="mb-4 flex items-center gap-4">
          <select
            value={selectedNode || ''}
            onChange={(e) => setSelectedNode(e.target.value)}
            className="flex h-9 rounded-md border border-input bg-transparent px-3 py-1 text-sm shadow-sm focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          >
            {nodes.map((n) => (
              <option key={n._id} value={n.node_id || n._id}>
                {n.name}
              </option>
            ))}
          </select>

          <div className="flex gap-1">
            {DATE_RANGES.map((r) => (
              <button
                key={r.days}
                onClick={() => setDays(r.days)}
                className={`rounded-md px-3 py-1.5 text-sm ${
                  days === r.days
                    ? 'bg-primary text-primary-foreground'
                    : 'border hover:bg-accent'
                }`}
              >
                {r.label}
              </button>
            ))}
          </div>
        </div>

        <div className="rounded-lg border bg-card p-4 shadow-sm">
          {historyLoading ? (
            <div className="flex h-64 items-center justify-center text-muted-foreground">
              Loading...
            </div>
          ) : history.length === 0 ? (
            <div className="flex h-64 items-center justify-center text-muted-foreground">
              No history data available
            </div>
          ) : (
            <ResponsiveContainer width="100%" height={300}>
              <LineChart data={history}>
                <CartesianGrid strokeDasharray="3 3" />
                <XAxis
                  dataKey="time"
                  tick={{ fontSize: 11 }}
                  tickFormatter={(v: string) => new Date(v).toLocaleDateString()}
                />
                <YAxis
                  tick={{ fontSize: 11 }}
                  tickFormatter={(v: number) => formatBytes(v)}
                />
                <Tooltip
                  labelFormatter={(v: string) => new Date(v).toLocaleString()}
                  formatter={(v: number) => formatBytes(v)}
                />
                <Line
                  type="monotone"
                  dataKey="used"
                  stroke="#3b82f6"
                  name="Used"
                  strokeWidth={2}
                  dot={false}
                />
                <Line
                  type="monotone"
                  dataKey="capacity"
                  stroke="#94a3b8"
                  name="Capacity"
                  strokeWidth={2}
                  strokeDasharray="5 5"
                  dot={false}
                />
              </LineChart>
            </ResponsiveContainer>
          )}
        </div>
      </div>
    </div>
  );
}
