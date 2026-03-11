import { useEffect, useCallback, useRef } from 'react';
import { isTauri } from '@/lib/platform';
import { getAuthToken, getBaseUrl } from '@/lib/api';

interface FileEntry {
  name: string;
  path: string;
  is_dir: boolean;
  file_id?: string;
}

interface UseFileListKeyboardOptions {
  files: FileEntry[];
  selectedIndex: number;
  setSelectedIndex: (index: number | ((prev: number) => number)) => void;
  onNavigate: (path: string) => void;
  onParent: () => void;
  onOpenDrawer: (index: number) => void;
  onCloseDrawer: () => void;
  drawerOpen: boolean;
}

async function openFileNative(fileId: string, fileName: string) {
  const baseUrl = getBaseUrl();
  const token = getAuthToken();
  if (!token || !fileId) return;

  try {
    // Get a download token
    const tokenRes = await fetch(`${baseUrl}/api/files/${fileId}/token`, {
      headers: { Authorization: `Bearer ${token}` },
    });
    if (!tokenRes.ok) return;
    const { token: downloadToken } = await tokenRes.json();

    // Download the file
    const downloadRes = await fetch(
      `${baseUrl}/api/files/${fileId}/download?token=${downloadToken}`,
    );
    if (!downloadRes.ok) return;
    const blob = await downloadRes.blob();

    // Write to temp dir and open with native app
    // Dynamic imports — only resolve in Tauri runtime
    const { tempDir } = await import('@tauri-apps/api/path' as string);
    const { writeFile } = await import('@tauri-apps/plugin-fs' as string);
    const { open } = await import('@tauri-apps/plugin-shell' as string);

    const tmpDir = await tempDir();
    const filePath = `${tmpDir}${fileName}`;
    const arrayBuf = await blob.arrayBuffer();
    await writeFile(filePath, new Uint8Array(arrayBuf));
    await open(filePath);
  } catch (err) {
    console.error('Failed to open file natively:', err);
  }
}

export function useFileListKeyboard({
  files,
  selectedIndex,
  setSelectedIndex,
  onNavigate,
  onParent,
  onOpenDrawer,
  onCloseDrawer,
  drawerOpen,
}: UseFileListKeyboardOptions) {
  const tableRef = useRef<HTMLTableSectionElement | null>(null);
  const tauri = isTauri();

  const scrollToIndex = useCallback((index: number) => {
    if (!tableRef.current) return;
    const rows = tableRef.current.querySelectorAll('tr');
    rows[index]?.scrollIntoView({ block: 'nearest' });
  }, []);

  useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
      // Skip when focus is in an input element
      const tag = (e.target as HTMLElement)?.tagName;
      if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return;

      switch (e.key) {
        case 'ArrowDown': {
          e.preventDefault();
          setSelectedIndex((prev: number) => {
            const next = Math.min(prev + 1, files.length - 1);
            scrollToIndex(next);
            return next;
          });
          break;
        }
        case 'ArrowUp': {
          e.preventDefault();
          setSelectedIndex((prev: number) => {
            const next = Math.max(prev - 1, 0);
            scrollToIndex(next);
            return next;
          });
          break;
        }
        case 'Enter': {
          e.preventDefault();
          const entry = files[selectedIndex];
          if (!entry) break;
          if (entry.is_dir) {
            onNavigate(entry.path);
          } else if (tauri && entry.file_id) {
            openFileNative(entry.file_id, entry.name);
          } else {
            // Web mode: open detail drawer
            onOpenDrawer(selectedIndex);
          }
          break;
        }
        case ' ': {
          e.preventDefault();
          if (files[selectedIndex]) {
            onOpenDrawer(selectedIndex);
          }
          break;
        }
        case 'Escape': {
          e.preventDefault();
          if (drawerOpen) {
            onCloseDrawer();
          } else {
            setSelectedIndex(-1);
          }
          break;
        }
        case 'ArrowRight': {
          const entry = files[selectedIndex];
          if (entry?.is_dir) {
            e.preventDefault();
            onNavigate(entry.path);
          }
          break;
        }
        case 'ArrowLeft': {
          e.preventDefault();
          onParent();
          break;
        }
        case 'Backspace': {
          if (e.metaKey || e.ctrlKey) {
            e.preventDefault();
            onParent();
          }
          break;
        }
      }
    }

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [files, selectedIndex, setSelectedIndex, onNavigate, onParent, onOpenDrawer, onCloseDrawer, drawerOpen, scrollToIndex, tauri]);

  return { tableRef };
}
