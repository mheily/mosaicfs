import { render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import DashboardPage from '@/pages/DashboardPage';
import { fullNode, nodeMinimal, fullNotification, notificationMinimal } from './fixtures';

const mockUseLiveQuery = vi.fn();

vi.mock('@/hooks/useLiveQuery', () => ({
  useLiveQuery: (selector: { type: string }) => mockUseLiveQuery(selector),
}));

function renderDashboard() {
  return render(
    <MemoryRouter>
      <DashboardPage />
    </MemoryRouter>,
  );
}

describe('DashboardPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders with populated data', () => {
    mockUseLiveQuery.mockImplementation((sel: { type: string }) => {
      if (sel.type === 'node') return { data: [fullNode], loading: false };
      if (sel.type === 'notification') return { data: [fullNotification], loading: false };
      if (sel.type === 'file') return { data: [{ _id: 'f1', type: 'file' }], loading: false };
      return { data: [], loading: false };
    });

    renderDashboard();
    expect(screen.getByText('Dashboard')).toBeInTheDocument();
    expect(screen.getByText('MacBook Pro')).toBeInTheDocument();
    expect(screen.getByText('1')).toBeInTheDocument(); // Total Files
  });

  it('renders with empty nodes and files', () => {
    mockUseLiveQuery.mockReturnValue({ data: [], loading: false });
    renderDashboard();
    expect(screen.getByText('No nodes registered')).toBeInTheDocument();
    expect(screen.getByText('0')).toBeInTheDocument(); // Total Files = 0
  });

  it('does not crash with minimal node data', () => {
    mockUseLiveQuery.mockImplementation((sel: { type: string }) => {
      if (sel.type === 'node') return { data: [nodeMinimal], loading: false };
      if (sel.type === 'notification') return { data: [], loading: false };
      if (sel.type === 'file') return { data: [], loading: false };
      return { data: [], loading: false };
    });

    expect(() => renderDashboard()).not.toThrow();
    expect(screen.getByText('Unknown Node')).toBeInTheDocument();
  });

  it('shows error banner for error notifications', () => {
    mockUseLiveQuery.mockImplementation((sel: { type: string }) => {
      if (sel.type === 'node') return { data: [], loading: false };
      if (sel.type === 'notification') {
        return {
          data: [{ ...fullNotification, severity: 'error' as const }],
          loading: false,
        };
      }
      if (sel.type === 'file') return { data: [], loading: false };
      return { data: [], loading: false };
    });

    renderDashboard();
    expect(screen.getByText('Crawl complete')).toBeInTheDocument();
  });

  it('shows warning banner when no errors but warnings exist', () => {
    mockUseLiveQuery.mockImplementation((sel: { type: string }) => {
      if (sel.type === 'node') return { data: [], loading: false };
      if (sel.type === 'notification') {
        return { data: [notificationMinimal], loading: false };
      }
      if (sel.type === 'file') return { data: [], loading: false };
      return { data: [], loading: false };
    });

    renderDashboard();
    expect(screen.getByText('Test warning')).toBeInTheDocument();
  });
});
