import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useLiveQuery } from '@/hooks/useLiveQuery';
import { formatRelative } from '@/lib/format';
import { Server } from 'lucide-react';
import { NodeBadge } from '@/components/NodeBadge';

interface NodeDoc {
  _id: string;
  type: string;
  name: string;
  status: string;
  platform?: string;
  last_heartbeat?: string;
  node_id?: string;
}

const STATUS_OPTIONS = ['all', 'online', 'offline', 'degraded'] as const;

export default function NodesPage() {
  const navigate = useNavigate();
  const { data: nodes, loading } = useLiveQuery<NodeDoc>({ type: 'node' });
  const [statusFilter, setStatusFilter] = useState<string>('all');

  const filtered =
    statusFilter === 'all' ? nodes : nodes.filter((n) => n.status === statusFilter);

  return (
    <div className="p-6">
      <div className="mb-4 flex items-center justify-between">
        <h1 className="text-2xl font-bold">Nodes</h1>
        <select
          value={statusFilter}
          onChange={(e) => setStatusFilter(e.target.value)}
          className="flex h-9 rounded-md border border-input bg-transparent px-3 py-1 text-sm shadow-sm focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        >
          {STATUS_OPTIONS.map((s) => (
            <option key={s} value={s}>
              {s === 'all' ? 'All statuses' : s.charAt(0).toUpperCase() + s.slice(1)}
            </option>
          ))}
        </select>
      </div>

      {loading ? (
        <p className="text-sm text-muted-foreground">Loading...</p>
      ) : filtered.length === 0 ? (
        <div className="flex flex-col items-center justify-center py-12 text-muted-foreground">
          <Server className="mb-2 h-8 w-8" />
          <p>No nodes found</p>
        </div>
      ) : (
        <table className="w-full text-sm">
          <thead>
            <tr className="border-b text-left text-muted-foreground">
              <th className="pb-2 font-medium">Name</th>
              <th className="pb-2 font-medium">Status</th>
              <th className="pb-2 font-medium">Platform</th>
              <th className="pb-2 font-medium">Last Heartbeat</th>
            </tr>
          </thead>
          <tbody>
            {filtered.map((node) => (
              <tr
                key={node._id}
                className="cursor-pointer border-b hover:bg-accent"
                onClick={() => navigate(`/nodes/${node.node_id || node._id}`)}
              >
                <td className="py-2 font-medium">{node.name}</td>
                <td className="py-2">
                  <NodeBadge status={node.status} />
                </td>
                <td className="py-2">{node.platform || '--'}</td>
                <td className="py-2">
                  {node.last_heartbeat ? formatRelative(node.last_heartbeat) : '--'}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
