import { useState, useEffect } from 'react';
import { api } from '@/lib/api';
import {
  Key,
  HardDrive,
  Puzzle,
  Settings,
  Info,
  Plus,
  Trash2,
  Copy,
  Check,
  Download,
  Upload,
  Eye,
  EyeOff,
} from 'lucide-react';

interface Credential {
  access_key_id: string;
  name: string;
  enabled: boolean;
  created_at: string;
}

interface CreateCredentialResponse {
  id: string;
  name: string;
  access_key_id: string;
  secret_key: string;
}

interface SystemInfo {
  version?: string;
  uptime?: string;
  pouchdb_doc_count?: number;
  pouchdb_update_seq?: number;
}

type TabId = 'credentials' | 'storage' | 'plugins' | 'general' | 'about';

const TABS: { id: TabId; label: string; icon: typeof Key }[] = [
  { id: 'credentials', label: 'Credentials', icon: Key },
  { id: 'storage', label: 'Storage Backends', icon: HardDrive },
  { id: 'plugins', label: 'Plugins', icon: Puzzle },
  { id: 'general', label: 'General', icon: Settings },
  { id: 'about', label: 'About', icon: Info },
];

export default function SettingsPage() {
  const [activeTab, setActiveTab] = useState<TabId>('credentials');

  return (
    <div className="p-6">
      <h1 className="mb-4 text-2xl font-bold">Settings</h1>

      {/* Tabs */}
      <div className="mb-6 flex border-b">
        {TABS.map((tab) => {
          const Icon = tab.icon;
          return (
            <button
              key={tab.id}
              onClick={() => setActiveTab(tab.id)}
              className={`flex items-center gap-2 border-b-2 px-4 py-2 text-sm font-medium transition-colors ${
                activeTab === tab.id
                  ? 'border-primary text-primary'
                  : 'border-transparent text-muted-foreground hover:text-foreground'
              }`}
            >
              <Icon className="h-4 w-4" />
              {tab.label}
            </button>
          );
        })}
      </div>

      {activeTab === 'credentials' && <CredentialsTab />}
      {activeTab === 'storage' && <ComingSoonTab title="Storage Backends" />}
      {activeTab === 'plugins' && <ComingSoonTab title="Plugins" />}
      {activeTab === 'general' && <GeneralTab />}
      {activeTab === 'about' && <AboutTab />}
    </div>
  );
}

function CredentialsTab() {
  const [credentials, setCredentials] = useState<Credential[]>([]);
  const [loading, setLoading] = useState(true);
  const [showCreate, setShowCreate] = useState(false);
  const [newName, setNewName] = useState('');
  const [createdSecret, setCreatedSecret] = useState<CreateCredentialResponse | null>(null);
  const [copied, setCopied] = useState(false);
  const [showSecret, setShowSecret] = useState(false);

  async function loadCredentials() {
    try {
      const data = await api<{ items: Credential[] }>('/api/credentials');
      setCredentials(data.items);
    } catch {
      setCredentials([]);
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    loadCredentials();
  }, []);

  async function handleCreate() {
    if (!newName.trim()) return;
    try {
      const result = await api<CreateCredentialResponse>('/api/credentials', {
        method: 'POST',
        body: JSON.stringify({ name: newName }),
      });
      setCreatedSecret(result);
      setNewName('');
      setShowCreate(false);
      await loadCredentials();
    } catch {
      // ignore
    }
  }

  async function handleToggleEnabled(cred: Credential) {
    try {
      await api(`/api/credentials/${cred.access_key_id}`, {
        method: 'PATCH',
        body: JSON.stringify({ enabled: !cred.enabled }),
      });
      await loadCredentials();
    } catch {
      // ignore
    }
  }

  async function handleDelete(id: string) {
    if (!confirm('Delete this credential?')) return;
    try {
      await api(`/api/credentials/${id}`, { method: 'DELETE' });
      await loadCredentials();
    } catch {
      // ignore
    }
  }

  function handleCopy(text: string) {
    navigator.clipboard.writeText(text);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  }

  if (loading) return <p className="text-sm text-muted-foreground">Loading...</p>;

  return (
    <div className="space-y-4">
      {/* Created secret display */}
      {createdSecret && (
        <div className="rounded-md border border-green-300 bg-green-50 p-4 dark:border-green-800 dark:bg-green-950">
          <p className="mb-2 text-sm font-medium text-green-800 dark:text-green-200">
            Credential created. Save the secret key now -- it will not be shown again.
          </p>
          <div className="space-y-2 text-sm">
            <div>
              <span className="text-muted-foreground">Access Key ID: </span>
              <code className="font-mono">{createdSecret.access_key_id}</code>
            </div>
            <div className="flex items-center gap-2">
              <span className="text-muted-foreground">Secret Key: </span>
              <code className="font-mono">
                {showSecret ? createdSecret.secret_key : '**********************'}
              </code>
              <button
                onClick={() => setShowSecret((v) => !v)}
                className="rounded p-1 hover:bg-accent"
              >
                {showSecret ? <EyeOff className="h-3.5 w-3.5" /> : <Eye className="h-3.5 w-3.5" />}
              </button>
              <button
                onClick={() => handleCopy(createdSecret.secret_key)}
                className="inline-flex items-center gap-1 rounded px-2 py-1 text-xs hover:bg-accent"
              >
                {copied ? <Check className="h-3.5 w-3.5" /> : <Copy className="h-3.5 w-3.5" />}
                {copied ? 'Copied' : 'Copy'}
              </button>
            </div>
          </div>
          <button
            onClick={() => setCreatedSecret(null)}
            className="mt-3 text-xs text-muted-foreground hover:underline"
          >
            Dismiss
          </button>
        </div>
      )}

      <div className="flex justify-end">
        <button
          onClick={() => setShowCreate(true)}
          className="inline-flex items-center gap-2 rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground shadow hover:bg-primary/90"
        >
          <Plus className="h-4 w-4" />
          Create Credential
        </button>
      </div>

      {/* Create Dialog */}
      {showCreate && (
        <div className="fixed inset-0 z-50 flex items-center justify-center">
          <div className="absolute inset-0 bg-black/50" onClick={() => setShowCreate(false)} />
          <div className="relative w-full max-w-sm rounded-lg bg-background p-6 shadow-lg">
            <h3 className="mb-4 text-lg font-semibold">Create Credential</h3>
            <form onSubmit={(e) => { e.preventDefault(); handleCreate(); }}>
            <div className="space-y-2">
              <label className="text-sm font-medium">Name</label>
              <input
                type="text"
                value={newName}
                onChange={(e) => setNewName(e.target.value)}
                className="flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm shadow-sm focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                placeholder="Credential name"
                autoFocus
              />
            </div>
            <div className="mt-4 flex justify-end gap-2">
              <button
                type="button"
                onClick={() => setShowCreate(false)}
                className="rounded-md border px-4 py-2 text-sm hover:bg-accent"
              >
                Cancel
              </button>
              <button
                type="submit"
                className="rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground shadow hover:bg-primary/90"
              >
                Create
              </button>
            </div>
            </form>
          </div>
        </div>
      )}

      {/* Credentials Table */}
      {credentials.length === 0 ? (
        <p className="py-8 text-center text-muted-foreground">No credentials</p>
      ) : (
        <table className="w-full text-sm">
          <thead>
            <tr className="border-b text-left text-muted-foreground">
              <th className="pb-2 font-medium">Name</th>
              <th className="pb-2 font-medium">Access Key ID</th>
              <th className="pb-2 font-medium">Enabled</th>
              <th className="pb-2 font-medium">Created</th>
              <th className="pb-2 font-medium">Actions</th>
            </tr>
          </thead>
          <tbody>
            {credentials.map((cred) => (
              <tr key={cred.access_key_id} className="border-b">
                <td className="py-2 font-medium">{cred.name}</td>
                <td className="py-2 font-mono text-xs">{cred.access_key_id}</td>
                <td className="py-2">
                  <button
                    onClick={() => handleToggleEnabled(cred)}
                    className={`relative inline-flex h-5 w-9 items-center rounded-full transition-colors ${
                      cred.enabled ? 'bg-primary' : 'bg-muted'
                    }`}
                  >
                    <span
                      className={`inline-block h-3.5 w-3.5 transform rounded-full bg-white shadow transition-transform ${
                        cred.enabled ? 'translate-x-4.5' : 'translate-x-0.5'
                      }`}
                    />
                  </button>
                </td>
                <td className="py-2">{new Date(cred.created_at).toLocaleDateString()}</td>
                <td className="py-2">
                  <button
                    onClick={() => handleDelete(cred.access_key_id)}
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
    </div>
  );
}

function ComingSoonTab({ title }: { title: string }) {
  return (
    <div className="rounded-lg border bg-card p-8 text-center shadow-sm">
      <p className="text-lg font-medium text-muted-foreground">{title}</p>
      <p className="mt-1 text-sm text-muted-foreground">Coming soon</p>
    </div>
  );
}

function GeneralTab() {
  return (
    <div className="max-w-lg space-y-4">
      <div className="space-y-2">
        <label className="text-sm font-medium">Instance Name</label>
        <input
          type="text"
          placeholder="My MosaicFS"
          className="flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm shadow-sm focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        />
      </div>
      <div className="space-y-2">
        <label className="text-sm font-medium">Sync Interval (seconds)</label>
        <input
          type="number"
          placeholder="60"
          className="flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm shadow-sm focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        />
      </div>
      <div className="space-y-2">
        <label className="text-sm font-medium">Log Level</label>
        <select className="flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm shadow-sm focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring">
          <option value="debug">Debug</option>
          <option value="info">Info</option>
          <option value="warn">Warning</option>
          <option value="error">Error</option>
        </select>
      </div>
      <button className="inline-flex h-9 items-center rounded-md bg-primary px-4 text-sm font-medium text-primary-foreground shadow hover:bg-primary/90">
        Save Settings
      </button>
    </div>
  );
}

interface BackupStatus {
  empty: boolean;
  document_count: number;
}

interface RestoreResult {
  ok: boolean;
  restored_count: number;
  errors: string[];
}

function AboutTab() {
  const [info, setInfo] = useState<SystemInfo | null>(null);
  const [backupStatus, setBackupStatus] = useState<BackupStatus | null>(null);
  const [restoreFile, setRestoreFile] = useState<File | null>(null);
  const [restoring, setRestoring] = useState(false);
  const [restoreResult, setRestoreResult] = useState<RestoreResult | null>(null);
  const [restoreError, setRestoreError] = useState<string | null>(null);
  const [wiping, setWiping] = useState(false);

  useEffect(() => {
    api<SystemInfo>('/api/system/info')
      .then(setInfo)
      .catch(() => setInfo(null));
    api<BackupStatus>('/api/system/backup/status')
      .then(setBackupStatus)
      .catch(() => setBackupStatus(null));
  }, []);

  function handleBackup(type: 'minimal' | 'full') {
    window.open(`/api/system/backup?type=${type}`, '_blank');
  }

  async function handleRestore() {
    if (!restoreFile) return;
    setRestoring(true);
    setRestoreResult(null);
    setRestoreError(null);
    try {
      const text = await restoreFile.text();
      const body = JSON.parse(text);
      const result = await api<RestoreResult>('/api/system/restore', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      });
      setRestoreResult(result);
      // Refresh backup status
      api<BackupStatus>('/api/system/backup/status')
        .then(setBackupStatus)
        .catch(() => {});
    } catch (e: unknown) {
      setRestoreError(e instanceof Error ? e.message : 'Restore failed');
    } finally {
      setRestoring(false);
    }
  }

  async function handleWipe() {
    if (!confirm('This will permanently delete ALL data. Are you sure?')) return;
    setWiping(true);
    try {
      await api('/api/system/data', {
        method: 'DELETE',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ confirm: 'DELETE_ALL_DATA' }),
      });
      // Refresh backup status
      const status = await api<BackupStatus>('/api/system/backup/status');
      setBackupStatus(status);
      setRestoreResult(null);
      setRestoreError(null);
    } catch (e: unknown) {
      alert(e instanceof Error ? e.message : 'Wipe failed');
    } finally {
      setWiping(false);
    }
  }

  return (
    <div className="space-y-6">
      <div className="rounded-lg border bg-card p-4 shadow-sm">
        <h3 className="mb-3 text-sm font-semibold">Instance Information</h3>
        <div className="space-y-2 text-sm">
          <div>
            <span className="text-muted-foreground">Version: </span>
            <span className="font-medium">{info?.version || '--'}</span>
          </div>
          <div>
            <span className="text-muted-foreground">Uptime: </span>
            <span className="font-medium">{info?.uptime || '--'}</span>
          </div>
        </div>
      </div>

      <div className="rounded-lg border bg-card p-4 shadow-sm">
        <h3 className="mb-3 text-sm font-semibold">PouchDB</h3>
        <div className="space-y-2 text-sm">
          <div>
            <span className="text-muted-foreground">Document Count: </span>
            <span className="font-medium">{info?.pouchdb_doc_count ?? '--'}</span>
          </div>
          <div>
            <span className="text-muted-foreground">Update Sequence: </span>
            <span className="font-medium">{info?.pouchdb_update_seq ?? '--'}</span>
          </div>
        </div>
      </div>

      <div className="rounded-lg border bg-card p-4 shadow-sm">
        <h3 className="mb-3 text-sm font-semibold">Backup</h3>
        <div className="flex gap-3">
          <button
            onClick={() => handleBackup('minimal')}
            className="inline-flex items-center gap-2 rounded-md border px-4 py-2 text-sm hover:bg-accent"
          >
            <Download className="h-4 w-4" />
            Minimal Backup
          </button>
          <button
            onClick={() => handleBackup('full')}
            className="inline-flex items-center gap-2 rounded-md border px-4 py-2 text-sm hover:bg-accent"
          >
            <Download className="h-4 w-4" />
            Full Backup
          </button>
        </div>
      </div>

      <div className="rounded-lg border bg-card p-4 shadow-sm">
        <h3 className="mb-3 text-sm font-semibold">Restore</h3>
        {restoreResult?.ok ? (
          <div className="space-y-2">
            <div className="rounded-md bg-green-50 p-3 text-sm text-green-800 dark:bg-green-950 dark:text-green-200">
              Restored {restoreResult.restored_count} documents. Restart all agents to complete recovery.
            </div>
            {restoreResult.errors.length > 0 && (
              <div className="rounded-md bg-yellow-50 p-3 text-sm text-yellow-800 dark:bg-yellow-950 dark:text-yellow-200">
                <p className="font-medium">Some documents had errors:</p>
                <ul className="mt-1 list-inside list-disc">
                  {restoreResult.errors.map((err, i) => (
                    <li key={i}>{err}</li>
                  ))}
                </ul>
              </div>
            )}
          </div>
        ) : backupStatus?.empty ? (
          <div className="space-y-3">
            <p className="text-sm text-muted-foreground">
              Upload a backup file to restore this instance.
            </p>
            <input
              type="file"
              accept=".json"
              onChange={(e) => setRestoreFile(e.target.files?.[0] ?? null)}
              className="block w-full text-sm text-muted-foreground file:mr-4 file:rounded-md file:border file:border-input file:bg-transparent file:px-4 file:py-2 file:text-sm file:font-medium hover:file:bg-accent"
            />
            {restoreError && (
              <div className="rounded-md bg-red-50 p-3 text-sm text-red-800 dark:bg-red-950 dark:text-red-200">
                {restoreError}
              </div>
            )}
            <button
              onClick={handleRestore}
              disabled={!restoreFile || restoring}
              className="inline-flex items-center gap-2 rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground shadow hover:bg-primary/90 disabled:opacity-50"
            >
              <Upload className="h-4 w-4" />
              {restoring ? 'Restoring...' : 'Restore'}
            </button>
          </div>
        ) : backupStatus ? (
          <p className="text-sm text-muted-foreground">
            Database has {backupStatus.document_count} documents. Restore is only available on an empty database.
          </p>
        ) : (
          <p className="text-sm text-muted-foreground">Loading status...</p>
        )}
      </div>

      {info && (info as SystemInfo & { developer_mode?: boolean }).developer_mode && (
        <div className="rounded-lg border border-red-300 bg-card p-4 shadow-sm dark:border-red-800">
          <h3 className="mb-3 text-sm font-semibold text-red-600 dark:text-red-400">Developer Mode</h3>
          <p className="mb-3 text-sm text-muted-foreground">
            Delete all data from this instance. This action cannot be undone.
          </p>
          <button
            onClick={handleWipe}
            disabled={wiping}
            className="inline-flex items-center gap-2 rounded-md bg-red-600 px-4 py-2 text-sm font-medium text-white shadow hover:bg-red-700 disabled:opacity-50"
          >
            <Trash2 className="h-4 w-4" />
            {wiping ? 'Deleting...' : 'Delete All Data'}
          </button>
        </div>
      )}
    </div>
  );
}
