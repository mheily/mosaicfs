import { useState, useEffect, useCallback } from 'react';
import { api } from '@/lib/api';
import { formatBytes, formatDate } from '@/lib/format';
import {
  Folder,
  File,
  ChevronRight,
  ChevronDown,
  Loader2,
} from 'lucide-react';
import { FileDetailDrawer } from '@/components/FileDetailDrawer';

interface VfsEntry {
  name: string;
  path: string;
  is_dir: boolean;
  size?: number;
  mtime?: string;
  node?: string;
}

interface TreeNode {
  name: string;
  path: string;
  children?: TreeNode[];
  loaded: boolean;
  expanded: boolean;
}

function DirectoryTreeItem({
  node,
  onSelect,
  onToggle,
  selectedPath,
}: {
  node: TreeNode;
  onSelect: (path: string) => void;
  onToggle: (path: string) => void;
  selectedPath: string;
}) {
  const isSelected = selectedPath === node.path;

  return (
    <div>
      <button
        onClick={() => {
          onToggle(node.path);
          onSelect(node.path);
        }}
        className={`flex w-full items-center gap-1 rounded px-2 py-1 text-left text-sm hover:bg-accent ${
          isSelected ? 'bg-accent font-medium' : ''
        }`}
      >
        {node.expanded ? (
          <ChevronDown className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
        ) : (
          <ChevronRight className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
        )}
        <Folder className="h-4 w-4 shrink-0 text-blue-500" />
        <span className="truncate">{node.name}</span>
      </button>
      {node.expanded && node.children && (
        <div className="ml-4">
          {node.children.map((child) => (
            <DirectoryTreeItem
              key={child.path}
              node={child}
              onSelect={onSelect}
              onToggle={onToggle}
              selectedPath={selectedPath}
            />
          ))}
        </div>
      )}
    </div>
  );
}

export default function FileBrowserPage() {
  const [currentPath, setCurrentPath] = useState('/');
  const [files, setFiles] = useState<VfsEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [tree, setTree] = useState<TreeNode[]>([
    { name: '/', path: '/', loaded: false, expanded: false },
  ]);
  const [selectedFile, setSelectedFile] = useState<VfsEntry | null>(null);

  const loadDirectory = useCallback(async (path: string) => {
    setLoading(true);
    try {
      const entries = await api<VfsEntry[]>(`/api/vfs?path=${encodeURIComponent(path)}`);
      setFiles(entries);
    } catch {
      setFiles([]);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    loadDirectory(currentPath);
  }, [currentPath, loadDirectory]);

  async function toggleTreeNode(path: string) {
    setTree((prev) => {
      const update = (nodes: TreeNode[]): TreeNode[] =>
        nodes.map((n) => {
          if (n.path === path) {
            return { ...n, expanded: !n.expanded };
          }
          if (n.children) {
            return { ...n, children: update(n.children) };
          }
          return n;
        });
      return update(prev);
    });

    // Lazy load children
    const findNode = (nodes: TreeNode[]): TreeNode | undefined => {
      for (const n of nodes) {
        if (n.path === path) return n;
        if (n.children) {
          const found = findNode(n.children);
          if (found) return found;
        }
      }
      return undefined;
    };

    const node = findNode(tree);
    if (node && !node.loaded) {
      try {
        const entries = await api<VfsEntry[]>(`/api/vfs?path=${encodeURIComponent(path)}`);
        const dirs = entries.filter((e) => e.is_dir);
        const children: TreeNode[] = dirs.map((d) => ({
          name: d.name,
          path: d.path,
          loaded: false,
          expanded: false,
        }));

        setTree((prev) => {
          const markLoaded = (nodes: TreeNode[]): TreeNode[] =>
            nodes.map((n) => {
              if (n.path === path) {
                return { ...n, loaded: true, expanded: true, children };
              }
              if (n.children) {
                return { ...n, children: markLoaded(n.children) };
              }
              return n;
            });
          return markLoaded(prev);
        });
      } catch {
        // ignore
      }
    }
  }

  const breadcrumbs = currentPath === '/' ? ['/'] : currentPath.split('/').filter(Boolean);

  return (
    <div className="flex h-full">
      {/* Left: Directory Tree */}
      <div className="w-64 shrink-0 overflow-auto border-r p-3">
        <h2 className="mb-2 text-sm font-semibold text-muted-foreground">Directories</h2>
        {tree.map((node) => (
          <DirectoryTreeItem
            key={node.path}
            node={node}
            onSelect={setCurrentPath}
            onToggle={toggleTreeNode}
            selectedPath={currentPath}
          />
        ))}
      </div>

      {/* Right: File Table */}
      <div className="flex-1 overflow-auto p-4">
        {/* Breadcrumbs */}
        <nav className="mb-4 flex items-center gap-1 text-sm">
          <button
            onClick={() => setCurrentPath('/')}
            className="text-primary hover:underline"
          >
            root
          </button>
          {breadcrumbs.map((seg, i) => {
            if (currentPath === '/' && i === 0) return null;
            const path = '/' + breadcrumbs.slice(0, i + 1).join('/');
            return (
              <span key={path} className="flex items-center gap-1">
                <ChevronRight className="h-3 w-3 text-muted-foreground" />
                <button
                  onClick={() => setCurrentPath(path)}
                  className="text-primary hover:underline"
                >
                  {seg}
                </button>
              </span>
            );
          })}
        </nav>

        {loading ? (
          <div className="flex items-center justify-center py-12">
            <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
          </div>
        ) : (
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b text-left text-muted-foreground">
                <th className="pb-2 font-medium">Name</th>
                <th className="pb-2 font-medium">Size</th>
                <th className="pb-2 font-medium">Modified</th>
                <th className="pb-2 font-medium">Node</th>
              </tr>
            </thead>
            <tbody>
              {files.map((entry) => (
                <tr
                  key={entry.path}
                  className="cursor-pointer border-b hover:bg-accent"
                  onClick={() => {
                    if (entry.is_dir) {
                      setCurrentPath(entry.path);
                    } else {
                      setSelectedFile(entry);
                    }
                  }}
                >
                  <td className="flex items-center gap-2 py-2">
                    {entry.is_dir ? (
                      <Folder className="h-4 w-4 text-blue-500" />
                    ) : (
                      <File className="h-4 w-4 text-muted-foreground" />
                    )}
                    {entry.name}
                  </td>
                  <td className="py-2">{entry.size != null ? formatBytes(entry.size) : '--'}</td>
                  <td className="py-2">{entry.mtime ? formatDate(entry.mtime) : '--'}</td>
                  <td className="py-2">{entry.node || '--'}</td>
                </tr>
              ))}
              {files.length === 0 && (
                <tr>
                  <td colSpan={4} className="py-8 text-center text-muted-foreground">
                    Empty directory
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        )}
      </div>

      {/* File Detail Drawer */}
      {selectedFile && (
        <FileDetailDrawer
          file={selectedFile}
          onClose={() => setSelectedFile(null)}
        />
      )}
    </div>
  );
}
