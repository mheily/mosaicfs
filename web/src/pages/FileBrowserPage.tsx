import { useState, useEffect, useCallback } from 'react';
import { api } from '@/lib/api';
import { formatBytes, formatDate } from '@/lib/format';
import { isTauri } from '@/lib/platform';
import { useFileListKeyboard } from '@/hooks/useFileListKeyboard';
import {
  Folder,
  File,
  ChevronRight,
  ChevronDown,
  Loader2,
} from 'lucide-react';
import { FileDetailDrawer, type FileDetail } from '@/components/FileDetailDrawer';

interface VfsEntry {
  name: string;
  path: string;
  is_dir: boolean;
  size?: number;
  mtime?: string;
  node?: string;
  file_id?: string;
  mime_type?: string;
}

interface VfsApiResponse {
  path: string;
  directories: Array<{ name: string; virtual_path: string }>;
  files: Array<{
    name: string;
    file_id: string;
    size?: number;
    mtime?: string;
    mime_type?: string;
    source?: { node_id?: string; export_path?: string };
  }>;
}

function mapVfsResponse(response: VfsApiResponse, _basePath: string): VfsEntry[] {
  const dirs: VfsEntry[] = response.directories.map((d) => ({
    name: d.name,
    path: d.virtual_path,
    is_dir: true,
  }));
  const files: VfsEntry[] = response.files.map((f) => ({
    name: f.name,
    path: f.file_id,
    is_dir: false,
    size: f.size,
    mtime: f.mtime,
    mime_type: f.mime_type,
    node: f.source?.node_id,
    file_id: f.file_id,
  }));
  return [...dirs, ...files];
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
  compact,
}: {
  node: TreeNode;
  onSelect: (path: string) => void;
  onToggle: (path: string) => void;
  selectedPath: string;
  compact?: boolean;
}) {
  const isSelected = selectedPath === node.path;
  const iconSize = compact ? 'h-3 w-3' : 'h-3.5 w-3.5';
  const folderSize = compact ? 'h-3.5 w-3.5' : 'h-4 w-4';

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
          <ChevronDown className={`${iconSize} shrink-0 text-muted-foreground`} />
        ) : (
          <ChevronRight className={`${iconSize} shrink-0 text-muted-foreground`} />
        )}
        <Folder className={`${folderSize} shrink-0 text-blue-500`} />
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
              compact={compact}
            />
          ))}
        </div>
      )}
    </div>
  );
}

export default function FileBrowserPage() {
  const compact = isTauri();
  const [currentPath, setCurrentPath] = useState('/');
  const [files, setFiles] = useState<VfsEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [tree, setTree] = useState<TreeNode[]>([
    { name: '/', path: '/', loaded: false, expanded: false },
  ]);
  const [selectedFile, setSelectedFile] = useState<FileDetail | null>(null);
  const [selectedIndex, setSelectedIndex] = useState(-1);

  const loadDirectory = useCallback(async (path: string) => {
    setLoading(true);
    try {
      const response = await api<VfsApiResponse>(`/api/vfs?path=${encodeURIComponent(path)}`);
      setFiles(mapVfsResponse(response, path));
    } catch {
      setFiles([]);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    loadDirectory(currentPath);
    setSelectedIndex(-1);
  }, [currentPath, loadDirectory]);

  // Auto-expand root "/" on mount so subdirectories are visible immediately
  const [rootExpanded, setRootExpanded] = useState(false);
  useEffect(() => {
    if (!rootExpanded) {
      setRootExpanded(true);
      (async () => {
        try {
          const response = await api<VfsApiResponse>(`/api/vfs?path=${encodeURIComponent('/')}`);
          const children: TreeNode[] = response.directories.map((d) => ({
            name: d.name,
            path: d.virtual_path,
            loaded: false,
            expanded: false,
          }));
          setTree([{ name: '/', path: '/', loaded: true, expanded: true, children }]);
        } catch {
          // ignore
        }
      })();
    }
  }, [rootExpanded]);

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
        const response = await api<VfsApiResponse>(`/api/vfs?path=${encodeURIComponent(path)}`);
        const children: TreeNode[] = response.directories.map((d) => ({
          name: d.name,
          path: d.virtual_path,
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

  const navigateToParent = useCallback(() => {
    if (currentPath !== '/') {
      const parts = currentPath.split('/').filter(Boolean);
      parts.pop();
      setCurrentPath(parts.length > 0 ? '/' + parts.join('/') : '/');
    }
  }, [currentPath]);

  function openFileDetail(entry: VfsEntry) {
    setSelectedFile({
      _id: entry.file_id ?? '',
      path: entry.name,
      export_path: entry.name,
      node_id: entry.node ?? '',
      size: entry.size ?? 0,
      mime_type: entry.mime_type ?? '',
      mtime: entry.mtime ?? '',
      labels: [],
    });
  }

  const { tableRef } = useFileListKeyboard({
    files,
    selectedIndex,
    setSelectedIndex,
    onNavigate: setCurrentPath,
    onParent: navigateToParent,
    onOpenDrawer: (index: number) => {
      const entry = files[index];
      if (entry) openFileDetail(entry);
    },
    onCloseDrawer: () => setSelectedFile(null),
    drawerOpen: !!selectedFile,
  });

  // Click handlers differ between Tauri (selection model) and web (immediate action)
  function handleRowClick(entry: VfsEntry, index: number) {
    if (compact) {
      // Tauri: single click selects
      setSelectedIndex(index);
    } else {
      // Web: single click opens/navigates
      if (entry.is_dir) {
        setCurrentPath(entry.path);
      } else {
        openFileDetail(entry);
      }
    }
  }

  function handleRowDoubleClick(entry: VfsEntry) {
    if (!compact) return; // Web mode uses single click
    if (entry.is_dir) {
      setCurrentPath(entry.path);
    } else {
      openFileDetail(entry);
    }
  }

  const breadcrumbs = currentPath === '/' ? ['/'] : currentPath.split('/').filter(Boolean);

  // Style classes that vary by mode
  const sidebarWidth = compact ? 'w-56' : 'w-64';
  const sidebarBg = compact ? 'bg-muted/20' : '';
  const cellPy = compact ? 'py-1' : 'py-2';
  const textSize = compact ? 'text-xs' : 'text-sm';
  const iconSize = compact ? 'h-3.5 w-3.5' : 'h-4 w-4';

  return (
    <div className="flex h-full">
      {/* Left: Directory Tree */}
      <div className={`${sidebarWidth} shrink-0 overflow-auto border-r p-3 ${sidebarBg}`}>
        {!compact && (
          <h2 className="mb-2 text-sm font-semibold text-muted-foreground">Directories</h2>
        )}
        {tree.map((node) => (
          <DirectoryTreeItem
            key={node.path}
            node={node}
            onSelect={setCurrentPath}
            onToggle={toggleTreeNode}
            selectedPath={currentPath}
            compact={compact}
          />
        ))}
      </div>

      {/* Right: File Table */}
      <div className="flex-1 overflow-auto p-4">
        {/* Breadcrumbs (only in web mode — FinderLayout has its own) */}
        {!compact && (
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
        )}

        {loading ? (
          <div className="flex items-center justify-center py-12">
            <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
          </div>
        ) : (
          <table className={`w-full ${textSize}`}>
            <thead>
              <tr className="border-b text-left text-muted-foreground">
                <th className="pb-2 font-medium">Name</th>
                <th className="pb-2 font-medium">Size</th>
                <th className="pb-2 font-medium">Modified</th>
                {!compact && <th className="pb-2 font-medium">Node</th>}
              </tr>
            </thead>
            <tbody ref={tableRef}>
              {files.map((entry, index) => (
                <tr
                  key={entry.path}
                  className={`cursor-pointer border-b hover:bg-accent ${
                    compact && index === selectedIndex
                      ? 'bg-primary/15 text-foreground'
                      : compact
                        ? 'even:bg-muted/30'
                        : ''
                  }`}
                  onClick={() => handleRowClick(entry, index)}
                  onDoubleClick={() => handleRowDoubleClick(entry)}
                >
                  <td className={`flex items-center gap-2 ${cellPy}`}>
                    {entry.is_dir ? (
                      <Folder className={`${iconSize} text-blue-500`} />
                    ) : (
                      <File className={`${iconSize} text-muted-foreground`} />
                    )}
                    {entry.name}
                  </td>
                  <td className={cellPy}>{entry.size != null ? formatBytes(entry.size) : '--'}</td>
                  <td className={cellPy}>{entry.mtime ? formatDate(entry.mtime) : '--'}</td>
                  {!compact && <td className={cellPy}>{entry.node || '--'}</td>}
                </tr>
              ))}
              {files.length === 0 && (
                <tr>
                  <td colSpan={compact ? 3 : 4} className="py-8 text-center text-muted-foreground">
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
