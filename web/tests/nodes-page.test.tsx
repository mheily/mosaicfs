import { render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import NodesPage from '@/pages/NodesPage';
import { fullNode, nodeMinimal, nodeNullFields } from './fixtures';

const mockUseLiveQuery = vi.fn();

vi.mock('@/hooks/useLiveQuery', () => ({
  useLiveQuery: (selector: { type: string }) => mockUseLiveQuery(selector),
}));

function renderNodes() {
  return render(
    <MemoryRouter>
      <NodesPage />
    </MemoryRouter>,
  );
}

describe('NodesPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders nodes with full data', () => {
    mockUseLiveQuery.mockReturnValue({ data: [fullNode], loading: false });
    renderNodes();
    expect(screen.getByText('MacBook Pro')).toBeInTheDocument();
    expect(screen.getByText('darwin')).toBeInTheDocument();
  });

  it('renders empty state', () => {
    mockUseLiveQuery.mockReturnValue({ data: [], loading: false });
    renderNodes();
    expect(screen.getByText('No nodes found')).toBeInTheDocument();
  });

  it('renders loading state', () => {
    mockUseLiveQuery.mockReturnValue({ data: [], loading: true });
    renderNodes();
    expect(screen.getByText('Loading...')).toBeInTheDocument();
  });

  it('does not crash with minimal node (no optional fields)', () => {
    mockUseLiveQuery.mockReturnValue({ data: [nodeMinimal], loading: false });
    expect(() => renderNodes()).not.toThrow();
    expect(screen.getByText('Unknown Node')).toBeInTheDocument();
    // Platform and heartbeat should show "--" fallback
    expect(screen.getAllByText('--').length).toBeGreaterThanOrEqual(2);
  });

  it('does not crash with null fields', () => {
    mockUseLiveQuery.mockReturnValue({ data: [nodeNullFields], loading: false });
    expect(() => renderNodes()).not.toThrow();
  });
});
