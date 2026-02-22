import { useState, useEffect } from 'react';
import { useLiveQuery } from '@/hooks/useLiveQuery';
import { api } from '@/lib/api';
import { Tag, BookOpen, Plus, X } from 'lucide-react';
import { FileDetailDrawer } from '@/components/FileDetailDrawer';

interface LabelAssignment {
  _id: string;
  type: string;
  path: string;
  labels: string[];
  node?: string;
}

interface LabelRule {
  _id: string;
  type: string;
  name: string;
  node_selector?: string;
  path_prefix: string;
  labels: string[];
  enabled: boolean;
}

export default function LabelsPage() {
  const [activeTab, setActiveTab] = useState<'assignments' | 'rules'>('assignments');

  return (
    <div className="p-6">
      <h1 className="mb-4 text-2xl font-bold">Labels</h1>

      {/* Tabs */}
      <div className="mb-4 flex border-b">
        <button
          onClick={() => setActiveTab('assignments')}
          className={`flex items-center gap-2 border-b-2 px-4 py-2 text-sm font-medium transition-colors ${
            activeTab === 'assignments'
              ? 'border-primary text-primary'
              : 'border-transparent text-muted-foreground hover:text-foreground'
          }`}
        >
          <Tag className="h-4 w-4" />
          Assignments
        </button>
        <button
          onClick={() => setActiveTab('rules')}
          className={`flex items-center gap-2 border-b-2 px-4 py-2 text-sm font-medium transition-colors ${
            activeTab === 'rules'
              ? 'border-primary text-primary'
              : 'border-transparent text-muted-foreground hover:text-foreground'
          }`}
        >
          <BookOpen className="h-4 w-4" />
          Rules
        </button>
      </div>

      {activeTab === 'assignments' ? <AssignmentsTab /> : <RulesTab />}
    </div>
  );
}

function AssignmentsTab() {
  const { data: assignments, loading } = useLiveQuery<LabelAssignment>({
    type: 'label_assignment',
  });
  const [selectedFile, setSelectedFile] = useState<{ path: string; name: string } | null>(null);

  if (loading) return <p className="text-sm text-muted-foreground">Loading...</p>;

  return (
    <>
      {assignments.length === 0 ? (
        <p className="py-8 text-center text-muted-foreground">No label assignments</p>
      ) : (
        <table className="w-full text-sm">
          <thead>
            <tr className="border-b text-left text-muted-foreground">
              <th className="pb-2 font-medium">Path</th>
              <th className="pb-2 font-medium">Labels</th>
              <th className="pb-2 font-medium">Node</th>
            </tr>
          </thead>
          <tbody>
            {assignments.map((a) => (
              <tr
                key={a._id}
                className="cursor-pointer border-b hover:bg-accent"
                onClick={() =>
                  setSelectedFile({
                    path: a.path,
                    name: a.path.split('/').pop() || a.path,
                  })
                }
              >
                <td className="py-2 font-medium">{a.path}</td>
                <td className="py-2">
                  <div className="flex flex-wrap gap-1">
                    {a.labels.map((l) => (
                      <span
                        key={l}
                        className="inline-flex rounded-full bg-primary/10 px-2 py-0.5 text-xs font-medium text-primary"
                      >
                        {l}
                      </span>
                    ))}
                  </div>
                </td>
                <td className="py-2">{a.node || '--'}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      {selectedFile && (
        <FileDetailDrawer file={selectedFile} onClose={() => setSelectedFile(null)} />
      )}
    </>
  );
}

function RulesTab() {
  const { data: rules, loading } = useLiveQuery<LabelRule>({ type: 'label_rule' });
  const [showEditor, setShowEditor] = useState(false);
  const [editingRule, setEditingRule] = useState<Partial<LabelRule>>({
    name: '',
    path_prefix: '',
    labels: [],
    enabled: true,
  });
  const [labelsInput, setLabelsInput] = useState('');

  async function handleToggleEnabled(rule: LabelRule) {
    try {
      await api(`/api/label-rules/${rule._id}`, {
        method: 'PATCH',
        body: JSON.stringify({ enabled: !rule.enabled }),
      });
    } catch {
      // ignore
    }
  }

  async function handleSaveRule() {
    try {
      const body = {
        ...editingRule,
        labels: labelsInput
          .split(',')
          .map((s) => s.trim())
          .filter(Boolean),
      };
      await api('/api/label-rules', {
        method: 'POST',
        body: JSON.stringify(body),
      });
      setShowEditor(false);
      setEditingRule({ name: '', path_prefix: '', labels: [], enabled: true });
      setLabelsInput('');
    } catch {
      // ignore
    }
  }

  if (loading) return <p className="text-sm text-muted-foreground">Loading...</p>;

  return (
    <>
      <div className="mb-4 flex justify-end">
        <button
          onClick={() => setShowEditor(true)}
          className="inline-flex items-center gap-2 rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground shadow hover:bg-primary/90"
        >
          <Plus className="h-4 w-4" />
          Add Rule
        </button>
      </div>

      {rules.length === 0 ? (
        <p className="py-8 text-center text-muted-foreground">No label rules</p>
      ) : (
        <table className="w-full text-sm">
          <thead>
            <tr className="border-b text-left text-muted-foreground">
              <th className="pb-2 font-medium">Name</th>
              <th className="pb-2 font-medium">Node Selector</th>
              <th className="pb-2 font-medium">Path Prefix</th>
              <th className="pb-2 font-medium">Labels</th>
              <th className="pb-2 font-medium">Enabled</th>
            </tr>
          </thead>
          <tbody>
            {rules.map((r) => (
              <tr key={r._id} className="border-b">
                <td className="py-2 font-medium">{r.name}</td>
                <td className="py-2">{r.node_selector || '--'}</td>
                <td className="py-2 font-mono text-xs">{r.path_prefix}</td>
                <td className="py-2">
                  <div className="flex flex-wrap gap-1">
                    {r.labels.map((l) => (
                      <span
                        key={l}
                        className="inline-flex rounded-full bg-primary/10 px-2 py-0.5 text-xs font-medium text-primary"
                      >
                        {l}
                      </span>
                    ))}
                  </div>
                </td>
                <td className="py-2">
                  <button
                    onClick={() => handleToggleEnabled(r)}
                    className={`relative inline-flex h-5 w-9 items-center rounded-full transition-colors ${
                      r.enabled ? 'bg-primary' : 'bg-muted'
                    }`}
                  >
                    <span
                      className={`inline-block h-3.5 w-3.5 transform rounded-full bg-white shadow transition-transform ${
                        r.enabled ? 'translate-x-4.5' : 'translate-x-0.5'
                      }`}
                    />
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      {/* Rule Editor Sheet */}
      {showEditor && (
        <div className="fixed inset-0 z-50 flex justify-end">
          <div className="absolute inset-0 bg-black/50" onClick={() => setShowEditor(false)} />
          <div className="relative w-full max-w-md bg-background p-6 shadow-lg">
            <div className="mb-4 flex items-center justify-between">
              <h3 className="text-lg font-semibold">Add Label Rule</h3>
              <button onClick={() => setShowEditor(false)}>
                <X className="h-5 w-5" />
              </button>
            </div>

            <div className="space-y-4">
              <div className="space-y-2">
                <label className="text-sm font-medium">Name</label>
                <input
                  type="text"
                  value={editingRule.name || ''}
                  onChange={(e) => setEditingRule((r) => ({ ...r, name: e.target.value }))}
                  className="flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm shadow-sm focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                />
              </div>

              <div className="space-y-2">
                <label className="text-sm font-medium">Path Prefix</label>
                <input
                  type="text"
                  value={editingRule.path_prefix || ''}
                  onChange={(e) => setEditingRule((r) => ({ ...r, path_prefix: e.target.value }))}
                  className="flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm shadow-sm focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  placeholder="/media/photos"
                />
              </div>

              <div className="space-y-2">
                <label className="text-sm font-medium">Labels (comma-separated)</label>
                <input
                  type="text"
                  value={labelsInput}
                  onChange={(e) => setLabelsInput(e.target.value)}
                  className="flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm shadow-sm focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  placeholder="photos, media, backup"
                />
              </div>

              <div className="flex items-center gap-2">
                <button
                  onClick={() =>
                    setEditingRule((r) => ({ ...r, enabled: !r.enabled }))
                  }
                  className={`relative inline-flex h-5 w-9 items-center rounded-full transition-colors ${
                    editingRule.enabled ? 'bg-primary' : 'bg-muted'
                  }`}
                >
                  <span
                    className={`inline-block h-3.5 w-3.5 transform rounded-full bg-white shadow transition-transform ${
                      editingRule.enabled ? 'translate-x-4.5' : 'translate-x-0.5'
                    }`}
                  />
                </button>
                <span className="text-sm">Enabled</span>
              </div>

              <button
                onClick={handleSaveRule}
                className="inline-flex h-9 w-full items-center justify-center rounded-md bg-primary px-4 text-sm font-medium text-primary-foreground shadow hover:bg-primary/90"
              >
                Save Rule
              </button>
            </div>
          </div>
        </div>
      )}
    </>
  );
}
