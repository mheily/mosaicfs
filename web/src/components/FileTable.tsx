import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import { NodeBadge } from '@/components/NodeBadge';
import { formatBytes, formatDate } from '@/lib/format';
import { ArrowUp, ArrowDown } from 'lucide-react';
import { cn } from '@/lib/utils';

interface FileEntry {
  _id: string;
  name: string;
  path: string;
  size: number;
  mtime: string;
  node_id: string;
  node_name?: string;
}

interface FileTableProps {
  files: FileEntry[];
  onFileClick: (file: FileEntry) => void;
  sortField: string;
  sortDir: 'asc' | 'desc';
  onSort: (field: string) => void;
}

function SortIcon({
  field,
  sortField,
  sortDir,
}: {
  field: string;
  sortField: string;
  sortDir: 'asc' | 'desc';
}) {
  if (field !== sortField) return null;
  return sortDir === 'asc' ? (
    <ArrowUp className="inline h-3 w-3" />
  ) : (
    <ArrowDown className="inline h-3 w-3" />
  );
}

function SortableHeader({
  field,
  label,
  sortField,
  sortDir,
  onSort,
  className,
}: {
  field: string;
  label: string;
  sortField: string;
  sortDir: 'asc' | 'desc';
  onSort: (field: string) => void;
  className?: string;
}) {
  return (
    <TableHead
      className={cn('cursor-pointer select-none', className)}
      onClick={() => onSort(field)}
    >
      <span className="inline-flex items-center gap-1">
        {label}
        <SortIcon field={field} sortField={sortField} sortDir={sortDir} />
      </span>
    </TableHead>
  );
}

export function FileTable({
  files,
  onFileClick,
  sortField,
  sortDir,
  onSort,
}: FileTableProps) {
  return (
    <Table>
      <TableHeader>
        <TableRow>
          <SortableHeader
            field="name"
            label="Name"
            sortField={sortField}
            sortDir={sortDir}
            onSort={onSort}
          />
          <SortableHeader
            field="size"
            label="Size"
            sortField={sortField}
            sortDir={sortDir}
            onSort={onSort}
            className="w-[100px]"
          />
          <SortableHeader
            field="mtime"
            label="Modified"
            sortField={sortField}
            sortDir={sortDir}
            onSort={onSort}
            className="w-[180px]"
          />
          <TableHead className="w-[120px]">Node</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {files.length === 0 ? (
          <TableRow>
            <TableCell
              colSpan={4}
              className="text-muted-foreground text-center"
            >
              No files found
            </TableCell>
          </TableRow>
        ) : (
          files.map((file) => (
            <TableRow
              key={file._id}
              className="cursor-pointer"
              onClick={() => onFileClick(file)}
            >
              <TableCell className="font-medium">{file.name}</TableCell>
              <TableCell>{formatBytes(file.size)}</TableCell>
              <TableCell>{formatDate(file.mtime)}</TableCell>
              <TableCell>
                <NodeBadge
                  status="online"
                  name={file.node_name || file.node_id}
                  nodeId={file.node_id}
                />
              </TableCell>
            </TableRow>
          ))
        )}
      </TableBody>
    </Table>
  );
}
