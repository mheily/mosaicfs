import { useState, useCallback } from 'react';
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from '@/components/ui/collapsible';
import { ChevronRight, ChevronDown, Folder, FolderOpen } from 'lucide-react';
import { cn } from '@/lib/utils';

interface TreeEntry {
  name: string;
  path: string;
  hasChildren: boolean;
}

interface DirectoryTreeProps {
  onSelect: (path: string) => void;
  selectedPath: string;
  fetchChildren: (path: string) => Promise<TreeEntry[]>;
}

function TreeNode({
  entry,
  depth,
  selectedPath,
  onSelect,
  fetchChildren,
}: {
  entry: TreeEntry;
  depth: number;
  selectedPath: string;
  onSelect: (path: string) => void;
  fetchChildren: (path: string) => Promise<TreeEntry[]>;
}) {
  const [open, setOpen] = useState(false);
  const [children, setChildren] = useState<TreeEntry[] | null>(null);
  const [loading, setLoading] = useState(false);
  const isSelected = selectedPath === entry.path;

  const handleToggle = useCallback(
    async (nextOpen: boolean) => {
      setOpen(nextOpen);
      if (nextOpen && children === null) {
        setLoading(true);
        try {
          const result = await fetchChildren(entry.path);
          setChildren(result);
        } catch {
          setChildren([]);
        } finally {
          setLoading(false);
        }
      }
    },
    [children, entry.path, fetchChildren],
  );

  const handleClick = () => {
    onSelect(entry.path);
  };

  if (!entry.hasChildren) {
    return (
      <button
        onClick={handleClick}
        className={cn(
          'flex w-full items-center gap-1.5 rounded px-2 py-1 text-left text-sm hover:bg-accent',
          isSelected && 'bg-accent font-medium',
        )}
        style={{ paddingLeft: `${depth * 16 + 8}px` }}
      >
        <Folder className="text-muted-foreground h-4 w-4 shrink-0" />
        <span className="truncate">{entry.name}</span>
      </button>
    );
  }

  return (
    <Collapsible open={open} onOpenChange={handleToggle}>
      <div
        className={cn(
          'flex items-center rounded hover:bg-accent',
          isSelected && 'bg-accent font-medium',
        )}
        style={{ paddingLeft: `${depth * 16 + 8}px` }}
      >
        <CollapsibleTrigger className="flex items-center p-1">
          {open ? (
            <ChevronDown className="h-3.5 w-3.5 shrink-0" />
          ) : (
            <ChevronRight className="h-3.5 w-3.5 shrink-0" />
          )}
        </CollapsibleTrigger>
        <button
          onClick={handleClick}
          className="flex flex-1 items-center gap-1.5 py-1 text-left text-sm"
        >
          {open ? (
            <FolderOpen className="text-muted-foreground h-4 w-4 shrink-0" />
          ) : (
            <Folder className="text-muted-foreground h-4 w-4 shrink-0" />
          )}
          <span className="truncate">{entry.name}</span>
        </button>
      </div>
      <CollapsibleContent>
        {loading && (
          <div
            className="text-muted-foreground px-2 py-1 text-xs"
            style={{ paddingLeft: `${(depth + 1) * 16 + 8}px` }}
          >
            Loading...
          </div>
        )}
        {children?.map((child) => (
          <TreeNode
            key={child.path}
            entry={child}
            depth={depth + 1}
            selectedPath={selectedPath}
            onSelect={onSelect}
            fetchChildren={fetchChildren}
          />
        ))}
      </CollapsibleContent>
    </Collapsible>
  );
}

export function DirectoryTree({
  onSelect,
  selectedPath,
  fetchChildren,
}: DirectoryTreeProps) {
  const [roots, setRoots] = useState<TreeEntry[] | null>(null);
  const [loading, setLoading] = useState(false);

  if (roots === null && !loading) {
    setLoading(true);
    fetchChildren('/')
      .then(setRoots)
      .catch(() => setRoots([]))
      .finally(() => setLoading(false));
  }

  if (loading && roots === null) {
    return (
      <div className="text-muted-foreground p-2 text-sm">Loading...</div>
    );
  }

  if (!roots || roots.length === 0) {
    return (
      <div className="text-muted-foreground p-2 text-sm">
        No directories found
      </div>
    );
  }

  return (
    <div className="space-y-0.5">
      {roots.map((entry) => (
        <TreeNode
          key={entry.path}
          entry={entry}
          depth={0}
          selectedPath={selectedPath}
          onSelect={onSelect}
          fetchChildren={fetchChildren}
        />
      ))}
    </div>
  );
}
