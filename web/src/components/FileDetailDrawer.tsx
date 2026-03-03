import { useEffect, useState } from 'react';
import {
  Sheet,
  SheetContent,
  SheetHeader,
  SheetTitle,
} from '@/components/ui/sheet';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from '@/components/ui/popover';
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from '@/components/ui/collapsible';
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/components/ui/tooltip';
import {
  Download,
  Plus,
  ChevronDown,
  ChevronRight,
  Pencil,
  Trash2,
  FolderInput,
} from 'lucide-react';
import { NodeBadge } from '@/components/NodeBadge';
import { LabelChip } from '@/components/LabelChip';
import { formatBytes, formatDate } from '@/lib/format';
import { api, getAuthToken } from '@/lib/api';

interface DownloadTokenResponse {
  token: string;
  url: string;
  expires_in: number;
}

export interface FileDetail {
  _id: string;
  path: string;
  export_path: string;
  node_id: string;
  size: number;
  mime_type: string;
  mtime: string;
  labels: string[];
}

interface FileDetailDrawerProps {
  file: FileDetail | null;
  onClose: () => void;
}

function filename(path: string): string {
  return path.split('/').pop() || path;
}

export function FileDetailDrawer({ file, onClose }: FileDetailDrawerProps) {
  const [labels, setLabels] = useState<string[]>([]);
  const [effectiveLabels, setEffectiveLabels] = useState<string[]>([]);
  const [labelInput, setLabelInput] = useState('');
  const [suggestions, setSuggestions] = useState<string[]>([]);
  const [popoverOpen, setPopoverOpen] = useState(false);
  const [annotationsOpen, setAnnotationsOpen] = useState(false);
  const [preview, setPreview] = useState<string | null>(null);
  // Signed download token URL — used for video/image/PDF preview and downloads.
  // No blob buffering needed; the browser streams directly from the server.
  const [tokenUrl, setTokenUrl] = useState<string | null>(null);

  useEffect(() => {
    setPreview(null);
    setLabels([]);
    setEffectiveLabels([]);
    setTokenUrl(null);

    if (!file) return;

    // Fetch a short-lived signed download URL for preview and the download button
    api<DownloadTokenResponse>(`/api/files/${file._id}/token`)
      .then((data) => setTokenUrl(data.url))
      .catch(() => {});

    // Fetch direct assignment labels (what the user has explicitly tagged)
    api<{ items: Array<{ labels: string[] }> }>(
      `/api/labels/assignments?file_id=${encodeURIComponent(file._id)}`,
    )
      .then((data) => setLabels(data.items[0]?.labels ?? []))
      .catch(() => {});

    // Fetch effective labels — direct + any matching label rules
    api<{ labels: string[] }>(
      `/api/labels/effective?file_id=${encodeURIComponent(file._id)}`,
    )
      .then((data) => setEffectiveLabels(data.labels ?? []))
      .catch(() => {});

    // Text preview: fetch content with auth header to display in <pre>
    const isText = file.mime_type?.startsWith('text/') || file.mime_type === 'application/json';
    if (!isText) return;

    const token = getAuthToken();
    fetch(`/api/files/${file._id}/content`, {
      headers: token ? { Authorization: `Bearer ${token}` } : {},
    })
      .then(async (r) => {
        if (!r.ok) return;
        setPreview(await r.text());
      })
      .catch(() => {});
  }, [file]);

  useEffect(() => {
    if (popoverOpen) {
      api<{ labels: string[] }>('/api/labels')
        .then((data) => setSuggestions(data.labels ?? []))
        .catch(() => setSuggestions([]));
    }
  }, [popoverOpen]);

  const addLabel = async (label: string) => {
    if (!file || !label.trim() || labels.includes(label.trim())) return;
    const trimmed = label.trim();
    const newLabels = [...labels, trimmed];
    try {
      await api('/api/labels/assignments', {
        method: 'PUT',
        body: JSON.stringify({ file_id: file._id, labels: newLabels }),
      });
      setLabels(newLabels);
      setLabelInput('');
      setPopoverOpen(false);
    } catch {
      // ignore
    }
  };

  const removeLabel = async (label: string) => {
    if (!file) return;
    const newLabels = labels.filter((l) => l !== label);
    try {
      await api('/api/labels/assignments', {
        method: 'PUT',
        body: JSON.stringify({ file_id: file._id, labels: newLabels }),
      });
      setLabels(newLabels);
    } catch {
      // ignore
    }
  };

  function handleDownload() {
    if (!file || !tokenUrl) return;
    window.open(tokenUrl, '_blank', 'noopener,noreferrer');
  }

  // Labels derived from matching rules — shown outlined, no remove button
  const labelSet = new Set(labels);
  const inheritedLabels = effectiveLabels.filter((l) => !labelSet.has(l));

  const filteredSuggestions = suggestions.filter(
    (s) =>
      s.toLowerCase().includes(labelInput.toLowerCase()) &&
      !labels.includes(s),
  );

  return (
    <Sheet open={!!file} onOpenChange={(open) => !open && onClose()}>
      <SheetContent className="w-[420px] overflow-y-auto sm:w-[480px]">
        {file && (
          <>
            <SheetHeader>
              <SheetTitle className="break-all">
                {filename(file.path)}
              </SheetTitle>
            </SheetHeader>

            <div className="mt-6 space-y-6">
              {/* Metadata */}
              <section className="space-y-2 text-sm">
                <h3 className="font-semibold">Metadata</h3>
                <div className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1.5">
                  <span className="text-muted-foreground">Path</span>
                  <span className="break-all font-mono text-xs">
                    {file.path}
                  </span>

                  <span className="text-muted-foreground">Export path</span>
                  <span className="break-all font-mono text-xs">
                    {file.export_path}
                  </span>

                  <span className="text-muted-foreground">Node</span>
                  <span>
                    <NodeBadge
                      status="online"
                      name={file.node_id}
                      nodeId={file.node_id}
                    />
                  </span>

                  <span className="text-muted-foreground">Size</span>
                  <span>{formatBytes(file.size)}</span>

                  <span className="text-muted-foreground">MIME type</span>
                  <span className="font-mono text-xs">{file.mime_type}</span>

                  <span className="text-muted-foreground">Modified</span>
                  <span>{formatDate(file.mtime)}</span>
                </div>
              </section>

              {/* Labels */}
              <section className="space-y-2">
                <h3 className="text-sm font-semibold">Labels</h3>
                <div className="flex flex-wrap gap-1.5">
                  {labels.map((label) => (
                    <LabelChip
                      key={label}
                      label={label}
                      inherited={false}
                      onRemove={() => removeLabel(label)}
                    />
                  ))}
                  {inheritedLabels.map((label) => (
                    <LabelChip
                      key={`rule-${label}`}
                      label={label}
                      inherited={true}
                    />
                  ))}
                  <Popover open={popoverOpen} onOpenChange={setPopoverOpen}>
                    <PopoverTrigger asChild>
                      <Button variant="outline" size="xs">
                        <Plus className="h-3 w-3" />
                        Add label
                      </Button>
                    </PopoverTrigger>
                    <PopoverContent className="w-56 p-2" align="start">
                      <Input
                        placeholder="Label name..."
                        value={labelInput}
                        onChange={(e) => setLabelInput(e.target.value)}
                        onKeyDown={(e) => {
                          if (e.key === 'Enter') addLabel(labelInput);
                        }}
                        className="mb-2"
                        autoFocus
                      />
                      {filteredSuggestions.length > 0 && (
                        <div className="max-h-32 space-y-0.5 overflow-y-auto">
                          {filteredSuggestions.map((s) => (
                            <button
                              key={s}
                              className="hover:bg-accent w-full rounded px-2 py-1 text-left text-sm"
                              onClick={() => addLabel(s)}
                            >
                              {s}
                            </button>
                          ))}
                        </div>
                      )}
                    </PopoverContent>
                  </Popover>
                </div>
              </section>

              {/* Annotations */}
              <Collapsible
                open={annotationsOpen}
                onOpenChange={setAnnotationsOpen}
              >
                <CollapsibleTrigger className="flex items-center gap-1 text-sm font-semibold">
                  {annotationsOpen ? (
                    <ChevronDown className="h-4 w-4" />
                  ) : (
                    <ChevronRight className="h-4 w-4" />
                  )}
                  Annotations
                </CollapsibleTrigger>
                <CollapsibleContent>
                  <p className="text-muted-foreground mt-2 text-sm">
                    No annotations yet
                  </p>
                </CollapsibleContent>
              </Collapsible>

              {/* Inline Preview */}
              {file.mime_type?.startsWith('video/') && tokenUrl && (
                <section className="space-y-2">
                  <h3 className="text-sm font-semibold">Preview</h3>
                  {/* eslint-disable-next-line jsx-a11y/media-has-caption */}
                  <video
                    src={tokenUrl}
                    controls
                    className="w-full rounded border"
                  />
                </section>
              )}
              {file.mime_type?.startsWith('image/') && tokenUrl && (
                <section className="space-y-2">
                  <h3 className="text-sm font-semibold">Preview</h3>
                  <img
                    src={tokenUrl}
                    alt={filename(file.path)}
                    className="max-h-64 rounded border object-contain"
                  />
                </section>
              )}
              {(file.mime_type?.startsWith('text/') ||
                file.mime_type === 'application/json') &&
                preview !== null && (
                  <section className="space-y-2">
                    <h3 className="text-sm font-semibold">Preview</h3>
                    <pre className="bg-muted max-h-64 overflow-auto rounded border p-3 font-mono text-xs">
                      {preview}
                    </pre>
                  </section>
                )}
              {file.mime_type === 'application/pdf' && tokenUrl && (
                <section className="space-y-2">
                  <h3 className="text-sm font-semibold">Preview</h3>
                  <iframe
                    src={tokenUrl}
                    className="h-96 w-full rounded border"
                    title="PDF preview"
                  />
                </section>
              )}

              {/* Actions */}
              <div className="flex flex-wrap gap-2">
                <Button variant="outline" size="sm" onClick={handleDownload} disabled={!tokenUrl}>
                  <Download className="h-4 w-4" />
                  Download
                </Button>

                <TooltipProvider>
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <span>
                        <Button variant="outline" size="sm" disabled>
                          <FolderInput className="h-4 w-4" />
                          Move
                        </Button>
                      </span>
                    </TooltipTrigger>
                    <TooltipContent>Coming in a future release</TooltipContent>
                  </Tooltip>
                </TooltipProvider>

                <TooltipProvider>
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <span>
                        <Button variant="outline" size="sm" disabled>
                          <Pencil className="h-4 w-4" />
                          Rename
                        </Button>
                      </span>
                    </TooltipTrigger>
                    <TooltipContent>Coming in a future release</TooltipContent>
                  </Tooltip>
                </TooltipProvider>

                <TooltipProvider>
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <span>
                        <Button variant="destructive" size="sm" disabled>
                          <Trash2 className="h-4 w-4" />
                          Delete
                        </Button>
                      </span>
                    </TooltipTrigger>
                    <TooltipContent>Coming in a future release</TooltipContent>
                  </Tooltip>
                </TooltipProvider>
              </div>
            </div>
          </>
        )}
      </SheetContent>
    </Sheet>
  );
}
