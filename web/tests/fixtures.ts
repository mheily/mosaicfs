/**
 * Shared test fixtures for boundary tests.
 *
 * "Full" fixtures have every field populated.
 * "Degraded" fixtures represent real-world edge cases we've hit:
 *   - PouchDB partial replication (missing fields)
 *   - API returning sparse objects
 *   - Null values from CouchDB views
 */

import type { FileDetail } from '@/components/FileDetailDrawer';

// ── FileDetail (used by FileDetailDrawer, SearchPage, LabelsPage) ──

export const fullFileDetail: FileDetail = {
  _id: 'file::abc123',
  path: '/photos/sunset.jpg',
  export_path: '/export/photos/sunset.jpg',
  node_id: 'node-laptop',
  size: 2048576,
  mime_type: 'image/jpeg',
  mtime: '2026-01-15T10:30:00Z',
  labels: ['photos', 'nature'],
};

export const fileDetailMissingPath: FileDetail = {
  ...fullFileDetail,
  path: undefined as unknown as string,
};

export const fileDetailNullFields: FileDetail = {
  ...fullFileDetail,
  path: null as unknown as string,
  mime_type: null as unknown as string,
  mtime: null as unknown as string,
  labels: null as unknown as string[],
};

export const fileDetailEmptyLabels: FileDetail = {
  ...fullFileDetail,
  labels: [],
};

export const fileDetailZeroSize: FileDetail = {
  ...fullFileDetail,
  size: 0,
};

// ── FileEntry (used by FileTable) ──

export interface FileEntryFixture {
  _id: string;
  name: string;
  size: number;
  mtime: string;
  source: {
    node_id: string;
    export_path: string;
    export_parent: string;
  };
}

export const fullFileEntry: FileEntryFixture = {
  _id: 'file::entry1',
  name: 'report.pdf',
  size: 102400,
  mtime: '2026-02-01T08:00:00Z',
  source: {
    node_id: 'node-server',
    export_path: '/docs/report.pdf',
    export_parent: '/docs',
  },
};

export const fileEntryUndefinedSource = {
  _id: 'file::entry2',
  name: 'orphan.txt',
  size: 512,
  mtime: '2026-01-20T12:00:00Z',
  source: undefined as unknown as FileEntryFixture['source'],
};

export const fileEntryUndefinedSize = {
  ...fullFileEntry,
  _id: 'file::entry3',
  size: undefined as unknown as number,
};

// ── NodeDoc (used by NodesPage, DashboardPage, StoragePage) ──

export interface NodeDocFixture {
  _id: string;
  type: string;
  name: string;
  status: string;
  platform?: string;
  last_heartbeat?: string;
  node_id?: string;
  storage?: Array<{ watch_paths_on_fs?: string[] }>;
  capacity?: number;
  used?: number;
  available?: number;
}

export const fullNode: NodeDocFixture = {
  _id: 'node::node-laptop',
  type: 'node',
  name: 'MacBook Pro',
  status: 'online',
  platform: 'darwin',
  last_heartbeat: '2026-02-28T14:30:00Z',
  node_id: 'node-laptop',
  storage: [{ watch_paths_on_fs: ['/Users/dev/Documents'] }],
  capacity: 1000000000000,
  used: 500000000000,
  available: 500000000000,
};

export const nodeMinimal: NodeDocFixture = {
  _id: 'node::node-unknown',
  type: 'node',
  name: 'Unknown Node',
  status: 'offline',
};

export const nodeNullFields = {
  _id: 'node::node-null',
  type: 'node',
  name: null as unknown as string,
  status: null as unknown as string,
  platform: null,
  last_heartbeat: null,
};

// ── LabelAssignment (used by LabelsPage) ──

export interface LabelAssignmentFixture {
  _id: string;
  type: string;
  file_id: string;
  labels: string[];
}

export const fullAssignment: LabelAssignmentFixture = {
  _id: 'label_file::abc123',
  type: 'label_assignment',
  file_id: 'file::abc123',
  labels: ['photos', 'important'],
};

export const assignmentEmptyLabels: LabelAssignmentFixture = {
  ...fullAssignment,
  _id: 'label_file::def456',
  labels: [],
};

export const assignmentUndefinedLabels = {
  ...fullAssignment,
  _id: 'label_file::ghi789',
  labels: undefined as unknown as string[],
};

// ── LabelRule (used by LabelsPage RulesTab) ──

export interface LabelRuleFixture {
  _id: string;
  type: string;
  name?: string;
  node_id?: string;
  path_prefix: string;
  glob?: string;
  labels: string[];
  enabled: boolean;
}

export const fullRule: LabelRuleFixture = {
  _id: 'label_rule::rule1',
  type: 'label_rule',
  name: 'Work documents',
  node_id: 'node-laptop',
  path_prefix: '/docs/',
  glob: '*.pdf',
  labels: ['work'],
  enabled: true,
};

export const ruleMinimal: LabelRuleFixture = {
  _id: 'label_rule::rule2',
  type: 'label_rule',
  path_prefix: '/',
  labels: ['catch-all'],
  enabled: false,
};

export const ruleUndefinedName = {
  ...fullRule,
  _id: 'label_rule::rule3',
  name: undefined,
  node_id: undefined,
};

// ── SearchApiItem (used by SearchPage) ──

export interface SearchApiItemFixture {
  id: string;
  name: string;
  source?: { node_id?: string; export_path?: string };
  size?: number;
  mtime?: string;
  mime_type?: string;
}

export const fullSearchItem: SearchApiItemFixture = {
  id: 'file::search1',
  name: 'sunset.jpg',
  source: { node_id: 'node-laptop', export_path: '/photos/sunset.jpg' },
  size: 2048576,
  mtime: '2026-01-15T10:30:00Z',
  mime_type: 'image/jpeg',
};

export const searchItemMinimal: SearchApiItemFixture = {
  id: 'file::search2',
  name: 'unknown-file',
};

export const searchItemNullSource: SearchApiItemFixture = {
  id: 'file::search3',
  name: 'no-source.txt',
  source: undefined,
  size: undefined,
  mtime: undefined,
  mime_type: undefined,
};

// ── NotificationDoc (used by NotificationPanel, DashboardPage) ──

export interface NotificationDocFixture {
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

export const fullNotification: NotificationDocFixture = {
  _id: 'notification::node-laptop::crawl_complete',
  type: 'notification',
  source: { node_id: 'node-laptop', component: 'crawler' },
  severity: 'info',
  status: 'active',
  title: 'Crawl complete',
  message: 'Initial crawl finished successfully.',
  actions: [{ label: 'View Files', api: '/api/files' }],
  condition_key: 'crawl_complete',
  first_seen: '2026-02-28T12:00:00Z',
  last_seen: '2026-02-28T12:00:00Z',
  occurrence_count: 1,
};

export const notificationMinimal: NotificationDocFixture = {
  _id: 'notification::node-x::test',
  type: 'notification',
  source: { node_id: 'node-x', component: 'system' },
  severity: 'warning',
  status: 'active',
  title: 'Test warning',
  message: 'Something needs attention.',
  condition_key: 'test_warning',
  first_seen: '2026-02-28T12:00:00Z',
  last_seen: '2026-02-28T12:00:00Z',
  occurrence_count: 1,
};

export const notificationUndefinedActions = {
  ...fullNotification,
  _id: 'notification::no-actions',
  actions: undefined,
  acknowledged_at: undefined,
  resolved_at: undefined,
};

export const notificationNullSource = {
  ...fullNotification,
  _id: 'notification::null-source',
  source: null as unknown as NotificationDocFixture['source'],
};
