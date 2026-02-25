import { useState } from 'react';
import { Bell, Check, CheckCheck, Clock, AlertTriangle, AlertCircle, Info } from 'lucide-react';
import { api } from '@/lib/api';
import { Button } from '@/components/ui/button';

export interface NotificationDoc {
  _id: string;
  type: 'notification';
  source: { node_id: string; component: string };
  severity: 'info' | 'warning' | 'error';
  status: 'active' | 'resolved' | 'acknowledged';
  title: string;
  message: string;
  actions?: { label: string; api: string }[];
  condition_key: string;
  first_seen: string;
  last_seen: string;
  occurrence_count: number;
  acknowledged_at?: string;
  resolved_at?: string;
}

function severityIcon(severity: string) {
  switch (severity) {
    case 'error':
      return <AlertCircle className="h-4 w-4 text-destructive" />;
    case 'warning':
      return <AlertTriangle className="h-4 w-4 text-amber-500" />;
    default:
      return <Info className="h-4 w-4 text-blue-500" />;
  }
}

function relativeTime(iso: string): string {
  const diff = Date.now() - new Date(iso).getTime();
  const mins = Math.floor(diff / 60000);
  if (mins < 1) return 'just now';
  if (mins < 60) return `${mins}m ago`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  return `${days}d ago`;
}

interface NotificationPanelProps {
  notifications: NotificationDoc[];
}

export function NotificationPanel({ notifications }: NotificationPanelProps) {
  const [acknowledging, setAcknowledging] = useState<string | null>(null);
  const [ackAllLoading, setAckAllLoading] = useState(false);
  const [showHistory, setShowHistory] = useState(false);
  const [history, setHistory] = useState<NotificationDoc[]>([]);
  const [historyLoading, setHistoryLoading] = useState(false);

  const active = notifications.filter(
    (n) => n.status === 'active' || n.status === 'acknowledged',
  );

  // Group by severity
  const errors = active.filter((n) => n.severity === 'error');
  const warnings = active.filter((n) => n.severity === 'warning');
  const infos = active.filter((n) => n.severity === 'info');

  async function handleAcknowledge(id: string) {
    const shortId = id.replace('notification::', '');
    setAcknowledging(id);
    try {
      await api(`/api/notifications/${encodeURIComponent(shortId)}/acknowledge`, {
        method: 'POST',
      });
    } catch {
      // Notification will sync back via PouchDB
    } finally {
      setAcknowledging(null);
    }
  }

  async function handleAcknowledgeAll() {
    setAckAllLoading(true);
    try {
      await api('/api/notifications/acknowledge-all', { method: 'POST' });
    } catch {
      // Will sync
    } finally {
      setAckAllLoading(false);
    }
  }

  async function handleViewHistory() {
    if (showHistory) {
      setShowHistory(false);
      return;
    }
    setHistoryLoading(true);
    try {
      const resp = await api<{ items: NotificationDoc[] }>('/api/notifications/history?limit=20');
      setHistory(resp.items);
    } catch {
      setHistory([]);
    } finally {
      setHistoryLoading(false);
      setShowHistory(true);
    }
  }

  function renderNotification(n: NotificationDoc) {
    return (
      <div
        key={n._id}
        className="flex items-start gap-3 rounded-md border p-3"
      >
        <div className="mt-0.5">{severityIcon(n.severity)}</div>
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2">
            <span className="text-sm font-medium">{n.title}</span>
            {n.occurrence_count > 1 && (
              <span className="text-xs text-muted-foreground">
                x{n.occurrence_count}
              </span>
            )}
          </div>
          <p className="text-xs text-muted-foreground mt-0.5">{n.message}</p>
          <div className="flex items-center gap-2 mt-1 text-xs text-muted-foreground">
            <span>{n.source?.component}</span>
            <span className="inline-flex items-center gap-1">
              <Clock className="h-3 w-3" />
              {relativeTime(n.last_seen)}
            </span>
          </div>
          {n.actions && n.actions.length > 0 && (
            <div className="flex gap-1 mt-2">
              {n.actions.map((action, i) => (
                <Button key={i} variant="outline" size="sm" className="h-6 text-xs" asChild>
                  <a href={action.api}>{action.label}</a>
                </Button>
              ))}
            </div>
          )}
        </div>
        {n.status === 'active' && (
          <Button
            variant="ghost"
            size="sm"
            className="h-7 px-2 shrink-0"
            disabled={acknowledging === n._id}
            onClick={() => handleAcknowledge(n._id)}
            title="Acknowledge"
          >
            <Check className="h-3.5 w-3.5" />
          </Button>
        )}
      </div>
    );
  }

  return (
    <div className="flex flex-col h-full">
      <div className="flex items-center justify-between p-4 border-b">
        <h2 className="font-semibold">Notifications</h2>
        {active.filter((n) => n.status === 'active').length > 0 && (
          <Button
            variant="outline"
            size="sm"
            className="h-7 text-xs gap-1"
            disabled={ackAllLoading}
            onClick={handleAcknowledgeAll}
          >
            <CheckCheck className="h-3.5 w-3.5" />
            Acknowledge all
          </Button>
        )}
      </div>

      <div className="flex-1 overflow-y-auto p-4 space-y-4">
        {active.length === 0 && !showHistory && (
          <div className="flex flex-col items-center justify-center py-8 text-muted-foreground">
            <Bell className="h-8 w-8 mb-2" />
            <p className="text-sm">No notifications</p>
          </div>
        )}

        {errors.length > 0 && (
          <div className="space-y-2">
            <h3 className="text-xs font-medium text-destructive uppercase tracking-wide">
              Errors ({errors.length})
            </h3>
            {errors.map(renderNotification)}
          </div>
        )}

        {warnings.length > 0 && (
          <div className="space-y-2">
            <h3 className="text-xs font-medium text-amber-500 uppercase tracking-wide">
              Warnings ({warnings.length})
            </h3>
            {warnings.map(renderNotification)}
          </div>
        )}

        {infos.length > 0 && (
          <div className="space-y-2">
            <h3 className="text-xs font-medium text-blue-500 uppercase tracking-wide">
              Info ({infos.length})
            </h3>
            {infos.map(renderNotification)}
          </div>
        )}

        {showHistory && (
          <div className="space-y-2 border-t pt-4">
            <h3 className="text-xs font-medium text-muted-foreground uppercase tracking-wide">
              History
            </h3>
            {historyLoading ? (
              <p className="text-xs text-muted-foreground">Loading...</p>
            ) : history.length === 0 ? (
              <p className="text-xs text-muted-foreground">No history</p>
            ) : (
              history.map((n) => (
                <div key={n._id ?? (n as unknown as { id: string }).id} className="flex items-start gap-3 rounded-md border p-3 opacity-60">
                  <div className="mt-0.5">{severityIcon(n.severity)}</div>
                  <div className="flex-1 min-w-0">
                    <span className="text-sm font-medium">{n.title}</span>
                    <p className="text-xs text-muted-foreground mt-0.5">{n.message}</p>
                    <span className="text-xs text-muted-foreground">
                      {n.resolved_at
                        ? `Resolved ${relativeTime(n.resolved_at)}`
                        : n.acknowledged_at
                          ? `Acknowledged ${relativeTime(n.acknowledged_at)}`
                          : ''}
                    </span>
                  </div>
                </div>
              ))
            )}
          </div>
        )}
      </div>

      <div className="border-t p-3">
        <Button
          variant="ghost"
          size="sm"
          className="w-full text-xs"
          onClick={handleViewHistory}
        >
          {showHistory ? 'Hide history' : 'View history'}
        </Button>
      </div>
    </div>
  );
}
