import { useState, useEffect } from 'react';
import { useParams } from 'react-router-dom';
import { useLiveDoc } from '@/hooks/useLiveDoc';
import { api } from '@/lib/api';
import { formatBytes, formatRelative } from '@/lib/format';
import { percentColor, percentBarColor } from '@/lib/format';
import {
  Server,
  HardDrive,
  Wifi,
  AlertTriangle,
  Plus,
  Trash2,
  Pencil,
  FolderOpen,
  Loader2,
} from 'lucide-react';
import {
  LineChart,
  Line,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
} from 'recharts';
import { NodeBadge } from '@/components/NodeBadge';

interface NodeDoc {
  _id: string;
  name: string;
  status: string;
  platform?: string;
  last_heartbeat?: string;
  node_id?: string;
}

interface AgentStatus {
  _id: string;
  uptime?: number;
  version?: string;
  pid?: number;
  cpu_usage?: number;
  memory_usage?: number;
}

interface StorageInfo {
  path: string;
  capacity: number;
  used: number;
  available: number;
}

interface UtilizationPoint {
  time: string;
  cpu: number;
  memory: number;
  disk: number;
}

interface WatchPath {
  path: string;
  recursive?: boolean;
}

interface NetworkMount {
  id: string;
  remote_path: string;
  local_path: string;
  node: string;
  status: string;
}

interface RecentError {
  timestamp: string;
  message: string;
  source?: string;
}

export default function NodeDetailPage() {
  const { nodeId } = useParams<{ nodeId: string }>();
  const { doc: node, loading: nodeLoading } = useLiveDoc<NodeDoc>(`node::${nodeId}`);
  const { doc: agentStatus } = useLiveDoc<AgentStatus>(`agent_status::${nodeId}`);

  const [storage, setStorage] = useState<StorageInfo[]>([]);
  const [utilization, setUtilization] = useState<UtilizationPoint[]>([]);
  const [watchPaths, setWatchPaths] = useState<WatchPath[]>([]);
  const [networkMounts, setNetworkMounts] = useState<NetworkMount[]>([]);
  const [recentErrors, setRecentErrors] = useState<RecentError[]>([]);

  // Mount editor
  const [showMountForm, setShowMountForm] = useState(false);
  const [editingMount, setEditingMount] = useState<Partial<NetworkMount>>({});

  useEffect(() => {
    if (!nodeId) return;

    api<StorageInfo[]>(`/api/nodes/${nodeId}/storage`).then(setStorage).catch(() => setStorage([]));
    api<UtilizationPoint[]>(`/api/nodes/${nodeId}/utilization?days=30`)
      .then(setUtilization)
      .catch(() => setUtilization([]));
    api<WatchPath[]>(`/api/nodes/${nodeId}/watch-paths`)
      .then(setWatchPaths)
      .catch(() => setWatchPaths([]));
    api<NetworkMount[]>(`/api/nodes/${nodeId}/network-mounts`)
      .then(setNetworkMounts)
      .catch(() => setNetworkMounts([]));
    api<RecentError[]>(`/api/nodes/${nodeId}/errors?limit=20`)
      .then(setRecentErrors)
      .catch(() => setRecentErrors([]));
  }, [nodeId]);

  async function handleSaveMount() {
    try {
      if (editingMount.id) {
        await api(`/api/nodes/${nodeId}/network-mounts/${editingMount.id}`, {
          method: 'PUT',
          body: JSON.stringify(editingMount),
        });
      } else {
        await api(`/api/nodes/${nodeId}/network-mounts`, {
          method: 'POST',
          body: JSON.stringify(editingMount),
        });
      }
      const updated = await api<NetworkMount[]>(`/api/nodes/${nodeId}/network-mounts`);
      setNetworkMounts(updated);
      setShowMountForm(false);
      setEditingMount({});
    } catch {
      // ignore
    }
  }

  async function handleDeleteMount(mountId: string) {
    try {
      await api(`/api/nodes/${nodeId}/network-mounts/${mountId}`, { method: 'DELETE' });
      setNetworkMounts((prev) => prev.filter((m) => m.id !== mountId));
    } catch {
      // ignore
    }
  }

  if (nodeLoading) {
    return (
      <div className="flex items-center justify-center py-12">
        <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
      </div>
    );
  }

  if (!node) {
    return (
      <div className="flex items-center justify-center py-12 text-muted-foreground">
        Node not found
      </div>
    );
  }

  return (
    <div className="space-y-6 p-6">
      {/* Header */}
      <div className="flex items-center gap-3">
        <Server className="h-6 w-6" />
        <h1 className="text-2xl font-bold">{node.name}</h1>
        <NodeBadge status={node.status} />
        {node.platform && (
          <span className="rounded bg-muted px-2 py-0.5 text-xs">{node.platform}</span>
        )}
      </div>

      {/* Agent Status */}
      {agentStatus && (
        <div className="rounded-lg border bg-card p-4 shadow-sm">
          <h2 className="mb-2 text-sm font-semibold text-muted-foreground">Agent Status</h2>
          <div className="grid grid-cols-2 gap-4 text-sm sm:grid-cols-4">
            {agentStatus.version && (
              <div>
                <span className="text-muted-foreground">Version:</span>{' '}
                <span className="font-medium">{agentStatus.version}</span>
              </div>
            )}
            {agentStatus.pid != null && (
              <div>
                <span className="text-muted-foreground">PID:</span>{' '}
                <span className="font-medium">{agentStatus.pid}</span>
              </div>
            )}
            {agentStatus.cpu_usage != null && (
              <div>
                <span className="text-muted-foreground">CPU:</span>{' '}
                <span className="font-medium">{agentStatus.cpu_usage.toFixed(1)}%</span>
              </div>
            )}
            {agentStatus.memory_usage != null && (
              <div>
                <span className="text-muted-foreground">Memory:</span>{' '}
                <span className="font-medium">{formatBytes(agentStatus.memory_usage)}</span>
              </div>
            )}
          </div>
        </div>
      )}

      {/* Storage Cards */}
      {storage.length > 0 && (
        <div>
          <h2 className="mb-3 text-lg font-semibold">Storage</h2>
          <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
            {storage.map((s) => {
              const pct = s.capacity > 0 ? Math.round((s.used / s.capacity) * 100) : 0;
              return (
                <div key={s.path} className="rounded-lg border bg-card p-4 shadow-sm">
                  <div className="mb-2 flex items-center gap-2">
                    <HardDrive className="h-4 w-4 text-muted-foreground" />
                    <span className="text-sm font-medium">{s.path}</span>
                  </div>
                  <div className="mb-1 flex justify-between text-xs text-muted-foreground">
                    <span>{formatBytes(s.used)} used</span>
                    <span className={percentColor(pct)}>{pct}%</span>
                  </div>
                  <div className="h-2 rounded-full bg-muted">
                    <div
                      className={`h-2 rounded-full ${percentBarColor(pct)}`}
                      style={{ width: `${pct}%` }}
                    />
                  </div>
                  <p className="mt-1 text-xs text-muted-foreground">
                    {formatBytes(s.available)} available of {formatBytes(s.capacity)}
                  </p>
                </div>
              );
            })}
          </div>
        </div>
      )}

      {/* Utilization Chart */}
      {utilization.length > 0 && (
        <div>
          <h2 className="mb-3 text-lg font-semibold">Utilization (30 days)</h2>
          <div className="rounded-lg border bg-card p-4 shadow-sm">
            <ResponsiveContainer width="100%" height={250}>
              <LineChart data={utilization}>
                <CartesianGrid strokeDasharray="3 3" />
                <XAxis
                  dataKey="time"
                  tick={{ fontSize: 11 }}
                  tickFormatter={(v: string) => new Date(v).toLocaleDateString()}
                />
                <YAxis tick={{ fontSize: 11 }} domain={[0, 100]} unit="%" />
                <Tooltip
                  labelFormatter={(v: string) => new Date(v).toLocaleString()}
                />
                <Line type="monotone" dataKey="cpu" stroke="#3b82f6" name="CPU" strokeWidth={2} dot={false} />
                <Line type="monotone" dataKey="memory" stroke="#8b5cf6" name="Memory" strokeWidth={2} dot={false} />
                <Line type="monotone" dataKey="disk" stroke="#f59e0b" name="Disk" strokeWidth={2} dot={false} />
              </LineChart>
            </ResponsiveContainer>
          </div>
        </div>
      )}

      {/* Watch Paths */}
      <div>
        <h2 className="mb-3 text-lg font-semibold">Watch Paths</h2>
        {watchPaths.length === 0 ? (
          <p className="text-sm text-muted-foreground">No watch paths configured</p>
        ) : (
          <div className="space-y-2">
            {watchPaths.map((wp, i) => (
              <div key={i} className="flex items-center gap-2 rounded-md border p-2 text-sm">
                <FolderOpen className="h-4 w-4 text-muted-foreground" />
                <span className="font-mono">{wp.path}</span>
                {wp.recursive && (
                  <span className="rounded bg-muted px-1.5 py-0.5 text-xs">recursive</span>
                )}
              </div>
            ))}
          </div>
        )}
      </div>

      {/* Network Mounts */}
      <div>
        <div className="mb-3 flex items-center justify-between">
          <h2 className="text-lg font-semibold">Network Mounts</h2>
          <button
            onClick={() => {
              setEditingMount({});
              setShowMountForm(true);
            }}
            className="inline-flex items-center gap-1 rounded-md bg-primary px-3 py-1.5 text-sm font-medium text-primary-foreground shadow hover:bg-primary/90"
          >
            <Plus className="h-3.5 w-3.5" />
            Add
          </button>
        </div>

        {networkMounts.length === 0 ? (
          <p className="text-sm text-muted-foreground">No network mounts</p>
        ) : (
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b text-left text-muted-foreground">
                <th className="pb-2 font-medium">Remote Path</th>
                <th className="pb-2 font-medium">Local Path</th>
                <th className="pb-2 font-medium">Node</th>
                <th className="pb-2 font-medium">Status</th>
                <th className="pb-2 font-medium">Actions</th>
              </tr>
            </thead>
            <tbody>
              {networkMounts.map((m) => (
                <tr key={m.id} className="border-b">
                  <td className="py-2 font-mono text-xs">{m.remote_path}</td>
                  <td className="py-2 font-mono text-xs">{m.local_path}</td>
                  <td className="py-2">{m.node}</td>
                  <td className="py-2">
                    <span
                      className={`inline-flex rounded-full px-2 py-0.5 text-xs font-medium ${
                        m.status === 'connected'
                          ? 'bg-green-100 text-green-700'
                          : 'bg-red-100 text-red-700'
                      }`}
                    >
                      {m.status}
                    </span>
                  </td>
                  <td className="flex gap-1 py-2">
                    <button
                      onClick={() => {
                        setEditingMount(m);
                        setShowMountForm(true);
                      }}
                      className="rounded p-1 hover:bg-accent"
                    >
                      <Pencil className="h-3.5 w-3.5" />
                    </button>
                    <button
                      onClick={() => handleDeleteMount(m.id)}
                      className="rounded p-1 text-destructive hover:bg-destructive/10"
                    >
                      <Trash2 className="h-3.5 w-3.5" />
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}

        {/* Mount Form */}
        {showMountForm && (
          <div className="mt-4 rounded-md border p-4">
            <h3 className="mb-3 text-sm font-semibold">
              {editingMount.id ? 'Edit' : 'Add'} Network Mount
            </h3>
            <div className="grid grid-cols-2 gap-3">
              <div className="space-y-1">
                <label className="text-xs text-muted-foreground">Remote Path</label>
                <input
                  value={editingMount.remote_path || ''}
                  onChange={(e) =>
                    setEditingMount((prev) => ({ ...prev, remote_path: e.target.value }))
                  }
                  className="flex h-8 w-full rounded-md border border-input bg-transparent px-2 text-sm"
                />
              </div>
              <div className="space-y-1">
                <label className="text-xs text-muted-foreground">Local Path</label>
                <input
                  value={editingMount.local_path || ''}
                  onChange={(e) =>
                    setEditingMount((prev) => ({ ...prev, local_path: e.target.value }))
                  }
                  className="flex h-8 w-full rounded-md border border-input bg-transparent px-2 text-sm"
                />
              </div>
              <div className="space-y-1">
                <label className="text-xs text-muted-foreground">Node</label>
                <input
                  value={editingMount.node || ''}
                  onChange={(e) =>
                    setEditingMount((prev) => ({ ...prev, node: e.target.value }))
                  }
                  className="flex h-8 w-full rounded-md border border-input bg-transparent px-2 text-sm"
                />
              </div>
            </div>
            <div className="mt-3 flex gap-2">
              <button
                onClick={handleSaveMount}
                className="rounded-md bg-primary px-4 py-1.5 text-sm font-medium text-primary-foreground shadow hover:bg-primary/90"
              >
                Save
              </button>
              <button
                onClick={() => {
                  setShowMountForm(false);
                  setEditingMount({});
                }}
                className="rounded-md border px-4 py-1.5 text-sm hover:bg-accent"
              >
                Cancel
              </button>
            </div>
          </div>
        )}
      </div>

      {/* Recent Errors */}
      <div>
        <h2 className="mb-3 text-lg font-semibold">Recent Errors</h2>
        {recentErrors.length === 0 ? (
          <p className="text-sm text-muted-foreground">No recent errors</p>
        ) : (
          <div className="space-y-2">
            {recentErrors.map((err, i) => (
              <div
                key={i}
                className="flex items-start gap-2 rounded-md border border-destructive/30 bg-destructive/5 p-3 text-sm"
              >
                <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0 text-destructive" />
                <div>
                  <p>{err.message}</p>
                  <p className="mt-1 text-xs text-muted-foreground">
                    {formatRelative(err.timestamp)}
                    {err.source && ` - ${err.source}`}
                  </p>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
