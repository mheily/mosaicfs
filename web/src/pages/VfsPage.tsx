import { useState, useEffect, useCallback } from 'react';
import { api } from '@/lib/api';
import { useDebounce } from '@/hooks/useDebounce';
import {
  Folder,
  ChevronRight,
  ChevronDown,
  Plus,
  Trash2,
  Pencil,
  ArrowUp,
  ArrowDown,
  X,
  Eye,
  Loader2,
} from 'lucide-react';
import { StepEditor } from '@/components/StepEditor';

interface VfsTreeNode {
  name: string;
  path: string;
  children?: VfsTreeNode[];
}

interface MountSource {
  node: string;
  path: string;
  strategy: string;
}

interface StepDef {
  op: string;
  value?: string;
  invert?: boolean;
  on_match?: string;
  [key: string]: unknown;
}

interface DirectoryInfo {
  path: string;
  mounts: MountSource[];
  steps: StepDef[];
}

interface PreviewResult {
  files: { name: string; path: string; node: string }[];
}

export default function VfsPage() {
  const [tree, setTree] = useState<VfsTreeNode[]>([]);
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [dirInfo, setDirInfo] = useState<DirectoryInfo | null>(null);
  const [loading, setLoading] = useState(false);

  // Mount editor sheet
  const [showMountEditor, setShowMountEditor] = useState(false);
  const [mounts, setMounts] = useState<MountSource[]>([]);
  const [steps, setSteps] = useState<StepDef[]>([]);

  // Preview
  const [preview, setPreview] = useState<PreviewResult | null>(null);
  const [previewLoading, setPreviewLoading] = useState(false);
  const debouncedSteps = useDebounce(steps, 500);

  // CRUD
  const [showCreateDialog, setShowCreateDialog] = useState(false);
  const [newDirName, setNewDirName] = useState('');
  const [showRenameDialog, setShowRenameDialog] = useState(false);
  const [renameName, setRenameName] = useState('');

  // Expanded directories
  const [expanded, setExpanded] = useState<Set<string>>(new Set(['/']));

  const loadTree = useCallback(async () => {
    try {
      const data = await api<VfsTreeNode[]>('/api/vfs/tree');
      setTree(data);
    } catch {
      setTree([]);
    }
  }, []);

  useEffect(() => {
    loadTree();
  }, [loadTree]);

  useEffect(() => {
    if (!selectedPath) {
      setDirInfo(null);
      return;
    }
    setLoading(true);
    api<DirectoryInfo>(`/api/vfs/directories/${encodeURIComponent(selectedPath)}`)
      .then((info) => {
        setDirInfo(info);
        setMounts(info.mounts || []);
        setSteps(info.steps || []);
      })
      .catch(() => setDirInfo(null))
      .finally(() => setLoading(false));
  }, [selectedPath]);

  // Live preview
  useEffect(() => {
    if (!selectedPath) return;
    setPreviewLoading(true);
    api<PreviewResult>(`/api/vfs/directories/${encodeURIComponent(selectedPath)}/preview`, {
      method: 'POST',
      body: JSON.stringify({ steps: debouncedSteps }),
    })
      .then(setPreview)
      .catch(() => setPreview(null))
      .finally(() => setPreviewLoading(false));
  }, [selectedPath, debouncedSteps]);

  function toggleExpand(path: string) {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  }

  function moveMountUp(index: number) {
    if (index === 0) return;
    setMounts((prev) => {
      const next = [...prev];
      [next[index - 1], next[index]] = [next[index], next[index - 1]];
      return next;
    });
  }

  function moveMountDown(index: number) {
    if (index >= mounts.length - 1) return;
    setMounts((prev) => {
      const next = [...prev];
      [next[index], next[index + 1]] = [next[index + 1], next[index]];
      return next;
    });
  }

  async function handleCreate() {
    if (!newDirName.trim()) return;
    const parentPath = selectedPath || '/';
    const newPath = parentPath === '/' ? `/${newDirName}` : `${parentPath}/${newDirName}`;
    try {
      await api('/api/vfs/directories', {
        method: 'POST',
        body: JSON.stringify({ path: newPath }),
      });
      await loadTree();
      setShowCreateDialog(false);
      setNewDirName('');
      setSelectedPath(newPath);
    } catch {
      // ignore
    }
  }

  async function handleRename() {
    if (!selectedPath || !renameName.trim()) return;
    try {
      await api(`/api/vfs/directories/${encodeURIComponent(selectedPath)}`, {
        method: 'PATCH',
        body: JSON.stringify({ name: renameName }),
      });
      await loadTree();
      setShowRenameDialog(false);
    } catch {
      // ignore
    }
  }

  async function handleDelete() {
    if (!selectedPath) return;
    if (!confirm(`Delete directory ${selectedPath}?`)) return;
    try {
      await api(`/api/vfs/directories/${encodeURIComponent(selectedPath)}`, {
        method: 'DELETE',
      });
      setSelectedPath(null);
      await loadTree();
    } catch {
      // ignore
    }
  }

  function renderTreeNode(node: VfsTreeNode, depth = 0) {
    const isExpanded = expanded.has(node.path);
    const isSelected = selectedPath === node.path;
    const hasChildren = node.children && node.children.length > 0;

    return (
      <div key={node.path}>
        <button
          onClick={() => {
            setSelectedPath(node.path);
            if (hasChildren) toggleExpand(node.path);
          }}
          className={`flex w-full items-center gap-1 rounded px-2 py-1 text-left text-sm hover:bg-accent ${
            isSelected ? 'bg-accent font-medium' : ''
          }`}
          style={{ paddingLeft: `${depth * 16 + 8}px` }}
        >
          {hasChildren ? (
            isExpanded ? (
              <ChevronDown className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
            ) : (
              <ChevronRight className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
            )
          ) : (
            <span className="w-3.5" />
          )}
          <Folder className="h-4 w-4 shrink-0 text-blue-500" />
          <span className="truncate">{node.name}</span>
        </button>
        {isExpanded &&
          node.children?.map((child) => renderTreeNode(child, depth + 1))}
      </div>
    );
  }

  return (
    <div className="flex h-full">
      {/* Left: VFS Tree */}
      <div className="w-64 shrink-0 overflow-auto border-r p-3">
        <div className="mb-2 flex items-center justify-between">
          <h2 className="text-sm font-semibold text-muted-foreground">VFS Tree</h2>
          <button
            onClick={() => setShowCreateDialog(true)}
            className="rounded p-1 hover:bg-accent"
            title="Create directory"
          >
            <Plus className="h-4 w-4" />
          </button>
        </div>
        {tree.map((node) => renderTreeNode(node))}
      </div>

      {/* Right: Directory info and editors */}
      <div className="flex-1 overflow-auto p-4">
        {!selectedPath ? (
          <div className="flex h-full items-center justify-center text-muted-foreground">
            Select a directory from the tree
          </div>
        ) : loading ? (
          <div className="flex items-center justify-center py-12">
            <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
          </div>
        ) : (
          <div className="space-y-6">
            {/* Header */}
            <div className="flex items-center justify-between">
              <h2 className="text-xl font-bold">{selectedPath}</h2>
              <div className="flex gap-2">
                <button
                  onClick={() => {
                    setRenameName(selectedPath.split('/').pop() || '');
                    setShowRenameDialog(true);
                  }}
                  className="inline-flex items-center gap-1 rounded-md border px-3 py-1.5 text-sm hover:bg-accent"
                >
                  <Pencil className="h-3.5 w-3.5" />
                  Rename
                </button>
                <button
                  onClick={handleDelete}
                  className="inline-flex items-center gap-1 rounded-md border border-destructive px-3 py-1.5 text-sm text-destructive hover:bg-destructive/10"
                >
                  <Trash2 className="h-3.5 w-3.5" />
                  Delete
                </button>
                <button
                  onClick={() => setShowMountEditor(true)}
                  className="inline-flex items-center gap-1 rounded-md bg-primary px-3 py-1.5 text-sm font-medium text-primary-foreground shadow hover:bg-primary/90"
                >
                  Edit Mounts
                </button>
              </div>
            </div>

            {/* Mount Sources */}
            <div>
              <h3 className="mb-2 text-sm font-semibold">Mount Sources</h3>
              {mounts.length === 0 ? (
                <p className="text-sm text-muted-foreground">No mount sources configured</p>
              ) : (
                <div className="space-y-2">
                  {mounts.map((m, i) => (
                    <div
                      key={i}
                      className="flex items-center gap-2 rounded-md border p-3 text-sm"
                    >
                      <div className="flex-1">
                        <span className="font-medium">{m.node}</span>
                        <span className="mx-2 text-muted-foreground">:</span>
                        <span className="font-mono">{m.path}</span>
                        <span className="ml-2 rounded bg-muted px-1.5 py-0.5 text-xs">
                          {m.strategy}
                        </span>
                      </div>
                      <button
                        onClick={() => moveMountUp(i)}
                        disabled={i === 0}
                        className="rounded p-1 hover:bg-accent disabled:opacity-30"
                      >
                        <ArrowUp className="h-3.5 w-3.5" />
                      </button>
                      <button
                        onClick={() => moveMountDown(i)}
                        disabled={i === mounts.length - 1}
                        className="rounded p-1 hover:bg-accent disabled:opacity-30"
                      >
                        <ArrowDown className="h-3.5 w-3.5" />
                      </button>
                    </div>
                  ))}
                </div>
              )}
            </div>

            {/* Step Pipeline */}
            <div>
              <h3 className="mb-2 text-sm font-semibold">Step Pipeline</h3>
              <StepEditor steps={steps} onChange={setSteps} />
            </div>

            {/* Live Preview */}
            <div>
              <h3 className="mb-2 flex items-center gap-2 text-sm font-semibold">
                <Eye className="h-4 w-4" />
                Live Preview
                {previewLoading && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
              </h3>
              <div className="max-h-64 overflow-auto rounded-md border">
                {preview && preview.files.length > 0 ? (
                  <table className="w-full text-sm">
                    <thead>
                      <tr className="border-b text-left text-muted-foreground">
                        <th className="px-3 py-2 font-medium">Name</th>
                        <th className="px-3 py-2 font-medium">Path</th>
                        <th className="px-3 py-2 font-medium">Node</th>
                      </tr>
                    </thead>
                    <tbody>
                      {preview.files.map((f, i) => (
                        <tr key={i} className="border-b">
                          <td className="px-3 py-1.5">{f.name}</td>
                          <td className="px-3 py-1.5 text-muted-foreground">{f.path}</td>
                          <td className="px-3 py-1.5">{f.node}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                ) : (
                  <p className="p-4 text-center text-sm text-muted-foreground">
                    {previewLoading ? 'Loading preview...' : 'No files match current pipeline'}
                  </p>
                )}
              </div>
            </div>
          </div>
        )}
      </div>

      {/* Mount Editor Sheet */}
      {showMountEditor && (
        <div className="fixed inset-0 z-50 flex justify-end">
          <div className="absolute inset-0 bg-black/50" onClick={() => setShowMountEditor(false)} />
          <div className="relative w-full max-w-lg overflow-auto bg-background p-6 shadow-lg">
            <div className="mb-4 flex items-center justify-between">
              <h3 className="text-lg font-semibold">Edit Mount Sources</h3>
              <button onClick={() => setShowMountEditor(false)}>
                <X className="h-5 w-5" />
              </button>
            </div>

            <div className="space-y-3">
              {mounts.map((m, i) => (
                <div key={i} className="rounded-md border p-3">
                  <div className="mb-2 flex items-center justify-between">
                    <span className="text-sm font-medium">Source {i + 1}</span>
                    <div className="flex gap-1">
                      <button
                        onClick={() => moveMountUp(i)}
                        disabled={i === 0}
                        className="rounded p-1 hover:bg-accent disabled:opacity-30"
                      >
                        <ArrowUp className="h-3.5 w-3.5" />
                      </button>
                      <button
                        onClick={() => moveMountDown(i)}
                        disabled={i === mounts.length - 1}
                        className="rounded p-1 hover:bg-accent disabled:opacity-30"
                      >
                        <ArrowDown className="h-3.5 w-3.5" />
                      </button>
                      <button
                        onClick={() =>
                          setMounts((prev) => prev.filter((_, idx) => idx !== i))
                        }
                        className="rounded p-1 text-destructive hover:bg-destructive/10"
                      >
                        <Trash2 className="h-3.5 w-3.5" />
                      </button>
                    </div>
                  </div>
                  <div className="grid grid-cols-3 gap-2">
                    <div>
                      <label className="text-xs text-muted-foreground">Node</label>
                      <input
                        value={m.node}
                        onChange={(e) =>
                          setMounts((prev) =>
                            prev.map((mt, idx) =>
                              idx === i ? { ...mt, node: e.target.value } : mt,
                            ),
                          )
                        }
                        className="flex h-8 w-full rounded-md border border-input bg-transparent px-2 text-sm"
                      />
                    </div>
                    <div>
                      <label className="text-xs text-muted-foreground">Path</label>
                      <input
                        value={m.path}
                        onChange={(e) =>
                          setMounts((prev) =>
                            prev.map((mt, idx) =>
                              idx === i ? { ...mt, path: e.target.value } : mt,
                            ),
                          )
                        }
                        className="flex h-8 w-full rounded-md border border-input bg-transparent px-2 text-sm"
                      />
                    </div>
                    <div>
                      <label className="text-xs text-muted-foreground">Strategy</label>
                      <select
                        value={m.strategy}
                        onChange={(e) =>
                          setMounts((prev) =>
                            prev.map((mt, idx) =>
                              idx === i ? { ...mt, strategy: e.target.value } : mt,
                            ),
                          )
                        }
                        className="flex h-8 w-full rounded-md border border-input bg-transparent px-2 text-sm"
                      >
                        <option value="merge">merge</option>
                        <option value="priority">priority</option>
                        <option value="mirror">mirror</option>
                      </select>
                    </div>
                  </div>
                </div>
              ))}

              <button
                onClick={() =>
                  setMounts((prev) => [...prev, { node: '', path: '', strategy: 'merge' }])
                }
                className="inline-flex w-full items-center justify-center gap-2 rounded-md border border-dashed px-4 py-2 text-sm text-muted-foreground hover:bg-accent"
              >
                <Plus className="h-4 w-4" />
                Add Mount Source
              </button>

              <button
                onClick={async () => {
                  try {
                    await api(
                      `/api/vfs/directories/${encodeURIComponent(selectedPath!)}`,
                      {
                        method: 'PATCH',
                        body: JSON.stringify({ mounts, steps }),
                      },
                    );
                    setShowMountEditor(false);
                  } catch {
                    // ignore
                  }
                }}
                className="inline-flex h-9 w-full items-center justify-center rounded-md bg-primary text-sm font-medium text-primary-foreground shadow hover:bg-primary/90"
              >
                Save
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Create Directory Dialog */}
      {showCreateDialog && (
        <div className="fixed inset-0 z-50 flex items-center justify-center">
          <div className="absolute inset-0 bg-black/50" onClick={() => setShowCreateDialog(false)} />
          <div className="relative w-full max-w-sm rounded-lg bg-background p-6 shadow-lg">
            <h3 className="mb-4 text-lg font-semibold">Create Directory</h3>
            <input
              type="text"
              value={newDirName}
              onChange={(e) => setNewDirName(e.target.value)}
              placeholder="Directory name"
              className="mb-4 flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm shadow-sm focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
              autoFocus
              onKeyDown={(e) => e.key === 'Enter' && handleCreate()}
            />
            <div className="flex justify-end gap-2">
              <button
                onClick={() => setShowCreateDialog(false)}
                className="rounded-md border px-4 py-2 text-sm hover:bg-accent"
              >
                Cancel
              </button>
              <button
                onClick={handleCreate}
                className="rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground shadow hover:bg-primary/90"
              >
                Create
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Rename Dialog */}
      {showRenameDialog && (
        <div className="fixed inset-0 z-50 flex items-center justify-center">
          <div className="absolute inset-0 bg-black/50" onClick={() => setShowRenameDialog(false)} />
          <div className="relative w-full max-w-sm rounded-lg bg-background p-6 shadow-lg">
            <h3 className="mb-4 text-lg font-semibold">Rename Directory</h3>
            <input
              type="text"
              value={renameName}
              onChange={(e) => setRenameName(e.target.value)}
              className="mb-4 flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm shadow-sm focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
              autoFocus
              onKeyDown={(e) => e.key === 'Enter' && handleRename()}
            />
            <div className="flex justify-end gap-2">
              <button
                onClick={() => setShowRenameDialog(false)}
                className="rounded-md border px-4 py-2 text-sm hover:bg-accent"
              >
                Cancel
              </button>
              <button
                onClick={handleRename}
                className="rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground shadow hover:bg-primary/90"
              >
                Rename
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
