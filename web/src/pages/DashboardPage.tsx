import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useLiveQuery } from '@/hooks/useLiveQuery';
import { Search, AlertTriangle, FileText, Server, Activity } from 'lucide-react';
import { NodeBadge } from '@/components/NodeBadge';
import type { NotificationDoc } from '@/components/NotificationPanel';

interface NodeDoc {
  _id: string;
  type: string;
  name: string;
  status: string;
  platform?: string;
  last_heartbeat?: string;
}

interface FileDoc {
  _id: string;
  type: string;
}

export default function DashboardPage() {
  const navigate = useNavigate();
  const [searchQuery, setSearchQuery] = useState('');

  const { data: nodes, loading: nodesLoading } = useLiveQuery<NodeDoc>({ type: 'node' });
  const { data: notifications } = useLiveQuery<NotificationDoc>({ type: 'notification' });
  const { data: files } = useLiveQuery<FileDoc>({ type: 'file' });

  const activeNotifications = notifications.filter(
    (n) => n.status === 'active',
  );
  const errorNotifications = activeNotifications.filter((n) => n.severity === 'error');
  const warningNotifications = activeNotifications.filter((n) => n.severity === 'warning');
  const [bannerDismissed, setBannerDismissed] = useState(false);

  function handleSearch(e: React.FormEvent) {
    e.preventDefault();
    if (searchQuery.trim()) {
      navigate(`/search?q=${encodeURIComponent(searchQuery.trim())}`);
    }
  }

  return (
    <div className="space-y-6 p-6">
      <h1 className="text-2xl font-bold">Dashboard</h1>

      {/* Error / warning banner */}
      {!bannerDismissed && errorNotifications.length > 0 && (
        <div className="flex items-center gap-2 rounded-md border border-destructive bg-destructive/10 p-4 text-destructive">
          <AlertTriangle className="h-5 w-5 shrink-0" />
          <div className="flex-1">
            <p className="font-medium">
              {errorNotifications[0].title}
              {errorNotifications.length > 1 &&
                ` (+${errorNotifications.length - 1} more)`}
            </p>
            <p className="text-sm">{errorNotifications[0].message}</p>
          </div>
          <button
            onClick={() => setBannerDismissed(true)}
            className="text-xs underline shrink-0"
          >
            Dismiss
          </button>
        </div>
      )}
      {!bannerDismissed && errorNotifications.length === 0 && warningNotifications.length > 0 && (
        <div className="flex items-center gap-2 rounded-md border border-amber-500 bg-amber-500/10 p-4 text-amber-700 dark:text-amber-400">
          <AlertTriangle className="h-5 w-5 shrink-0" />
          <div className="flex-1">
            <p className="font-medium">
              {warningNotifications[0].title}
              {warningNotifications.length > 1 &&
                ` (+${warningNotifications.length - 1} more)`}
            </p>
            <p className="text-sm">{warningNotifications[0].message}</p>
          </div>
          <button
            onClick={() => setBannerDismissed(true)}
            className="text-xs underline shrink-0"
          >
            Dismiss
          </button>
        </div>
      )}

      {/* Search bar */}
      <form onSubmit={handleSearch} className="flex gap-2">
        <div className="relative flex-1">
          <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
          <input
            type="text"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder="Search files..."
            className="flex h-9 w-full rounded-md border border-input bg-transparent pl-9 pr-3 py-1 text-sm shadow-sm placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          />
        </div>
        <button
          type="submit"
          className="inline-flex h-9 items-center rounded-md bg-primary px-4 text-sm font-medium text-primary-foreground shadow hover:bg-primary/90"
        >
          Search
        </button>
      </form>

      {/* System totals */}
      <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
        <div className="rounded-lg border bg-card p-4 shadow-sm">
          <div className="flex items-center gap-2 text-muted-foreground">
            <FileText className="h-4 w-4" />
            <span className="text-sm font-medium">Total Files</span>
          </div>
          <p className="mt-2 text-3xl font-bold">{files.length}</p>
        </div>

        <div className="rounded-lg border bg-card p-4 shadow-sm">
          <div className="flex items-center gap-2 text-muted-foreground">
            <Server className="h-4 w-4" />
            <span className="text-sm font-medium">Total Nodes</span>
          </div>
          <p className="mt-2 text-3xl font-bold">{nodes.length}</p>
        </div>

        <div className="rounded-lg border bg-card p-4 shadow-sm">
          <div className="flex items-center gap-2 text-muted-foreground">
            <Server className="h-4 w-4" />
            <span className="text-sm font-medium">Nodes Online</span>
          </div>
          <p className="mt-2 text-3xl font-bold">
            {nodes.filter((n) => n.status === 'online').length}
          </p>
        </div>
      </div>

      {/* Node health strip */}
      <div>
        <h2 className="mb-3 text-lg font-semibold">Node Health</h2>
        {nodesLoading ? (
          <p className="text-sm text-muted-foreground">Loading...</p>
        ) : nodes.length === 0 ? (
          <p className="text-sm text-muted-foreground">No nodes registered</p>
        ) : (
          <div className="flex flex-wrap gap-3">
            {nodes.map((node) => (
              <div key={node._id} className="rounded-lg border bg-card p-3 shadow-sm">
                <div className="flex items-center gap-2">
                  <span className="text-sm font-medium">{node.name}</span>
                  <NodeBadge status={node.status} />
                </div>
                {node.platform && (
                  <p className="mt-1 text-xs text-muted-foreground">{node.platform}</p>
                )}
              </div>
            ))}
          </div>
        )}
      </div>

      {/* Recent activity stub */}
      <div>
        <h2 className="mb-3 text-lg font-semibold">Recent Activity</h2>
        <div className="rounded-lg border bg-card p-6 shadow-sm">
          <div className="flex items-center justify-center gap-2 text-muted-foreground">
            <Activity className="h-5 w-5" />
            <span className="text-sm">Activity feed coming soon</span>
          </div>
        </div>
      </div>
    </div>
  );
}
