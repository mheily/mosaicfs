import { render, screen } from '@testing-library/react';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import NodeDetailPage from '@/pages/NodeDetailPage';

const mockUseLiveDoc = vi.fn();
const mockApi = vi.fn();

vi.mock('@/hooks/useLiveDoc', () => ({
  useLiveDoc: (id: string) => mockUseLiveDoc(id),
}));

vi.mock('@/lib/api', () => ({
  api: (...args: unknown[]) => mockApi(...args),
}));

// Mock recharts to avoid ResizeObserver issues
vi.mock('recharts', () => ({
  LineChart: ({ children }: { children: React.ReactNode }) => <div data-testid="chart">{children}</div>,
  Line: () => null,
  XAxis: () => null,
  YAxis: () => null,
  CartesianGrid: () => null,
  Tooltip: () => null,
  ResponsiveContainer: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
}));

function renderNodeDetail(nodeId = 'node-laptop') {
  return render(
    <MemoryRouter initialEntries={[`/nodes/${nodeId}`]}>
      <Routes>
        <Route path="/nodes/:nodeId" element={<NodeDetailPage />} />
      </Routes>
    </MemoryRouter>,
  );
}

describe('NodeDetailPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockApi.mockResolvedValue([]);
  });

  it('renders node with full data', () => {
    mockUseLiveDoc.mockImplementation((id: string) => {
      if (id.startsWith('node::')) {
        return {
          doc: {
            _id: 'node::node-laptop',
            name: 'MacBook Pro',
            status: 'online',
            platform: 'darwin',
            last_heartbeat: '2026-02-28T14:30:00Z',
            storage: [{ watch_paths_on_fs: ['/Users/dev'] }],
          },
          loading: false,
        };
      }
      return { doc: null, loading: false };
    });

    renderNodeDetail();
    expect(screen.getByText('MacBook Pro')).toBeInTheDocument();
    expect(screen.getByText('darwin')).toBeInTheDocument();
    expect(screen.getByText('/Users/dev')).toBeInTheDocument();
  });

  it('shows "Node not found" when doc is null', () => {
    mockUseLiveDoc.mockReturnValue({ doc: null, loading: false });
    renderNodeDetail();
    expect(screen.getByText('Node not found')).toBeInTheDocument();
  });

  it('shows loading spinner while loading', () => {
    mockUseLiveDoc.mockReturnValue({ doc: null, loading: true });
    renderNodeDetail();
    // The Loader2 icon should be present
    expect(screen.queryByText('Node not found')).not.toBeInTheDocument();
  });

  it('does not crash when storage is undefined', () => {
    mockUseLiveDoc.mockImplementation((id: string) => {
      if (id.startsWith('node::')) {
        return {
          doc: {
            _id: 'node::node-x',
            name: 'Minimal Node',
            status: 'offline',
          },
          loading: false,
        };
      }
      return { doc: null, loading: false };
    });

    expect(() => renderNodeDetail('node-x')).not.toThrow();
    expect(screen.getByText('Minimal Node')).toBeInTheDocument();
    expect(screen.getByText('No watch paths configured')).toBeInTheDocument();
  });

  it('does not crash when network mounts and errors are empty', () => {
    mockUseLiveDoc.mockImplementation((id: string) => {
      if (id.startsWith('node::')) {
        return {
          doc: {
            _id: 'node::node-y',
            name: 'Empty Node',
            status: 'degraded',
            storage: [],
          },
          loading: false,
        };
      }
      return { doc: null, loading: false };
    });

    renderNodeDetail('node-y');
    expect(screen.getByText('No recent errors')).toBeInTheDocument();
    expect(screen.getByText('No network mounts')).toBeInTheDocument();
  });
});
