import { render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import LabelsPage from '@/pages/LabelsPage';
import {
  fullAssignment,
  assignmentEmptyLabels,
  fullRule,
  ruleMinimal,
  ruleUndefinedName,
} from './fixtures';

const mockUseLiveQuery = vi.fn();

vi.mock('@/hooks/useLiveQuery', () => ({
  useLiveQuery: (selector: { type: string }) => mockUseLiveQuery(selector),
}));

vi.mock('@/lib/pouchdb', () => ({
  getDB: () => ({
    get: vi.fn().mockRejectedValue({ status: 404 }),
  }),
}));

vi.mock('@/lib/api', () => ({
  api: vi.fn().mockResolvedValue([]),
  getAuthToken: vi.fn().mockReturnValue(null),
}));

function renderLabels() {
  return render(
    <MemoryRouter>
      <LabelsPage />
    </MemoryRouter>,
  );
}

describe('LabelsPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  describe('AssignmentsTab', () => {
    it('renders assignments with full data', () => {
      mockUseLiveQuery.mockImplementation((sel: { type: string }) => {
        if (sel.type === 'label_assignment') return { data: [fullAssignment], loading: false };
        return { data: [], loading: false };
      });

      renderLabels();
      // Assignments tab is active by default
      expect(screen.getByText('photos')).toBeInTheDocument();
      expect(screen.getByText('important')).toBeInTheDocument();
    });

    it('renders empty state', () => {
      mockUseLiveQuery.mockReturnValue({ data: [], loading: false });
      renderLabels();
      expect(screen.getByText('No label assignments')).toBeInTheDocument();
    });

    it('renders loading state', () => {
      mockUseLiveQuery.mockReturnValue({ data: [], loading: true });
      renderLabels();
      expect(screen.getByText('Loading...')).toBeInTheDocument();
    });

    it('does not crash with empty labels array', () => {
      mockUseLiveQuery.mockImplementation((sel: { type: string }) => {
        if (sel.type === 'label_assignment') return { data: [assignmentEmptyLabels], loading: false };
        return { data: [], loading: false };
      });

      expect(() => renderLabels()).not.toThrow();
    });

    it('shows "--" when file doc is not found (no matching PouchDB doc)', () => {
      mockUseLiveQuery.mockImplementation((sel: { type: string }) => {
        if (sel.type === 'label_assignment') return { data: [fullAssignment], loading: false };
        return { data: [], loading: false };
      });

      renderLabels();
      // Since PouchDB get mock returns 404, the path and node columns show "--"
      const dashes = screen.getAllByText('--');
      expect(dashes.length).toBeGreaterThanOrEqual(2);
    });
  });

  describe('RulesTab', () => {
    it('renders rules with full data', async () => {
      mockUseLiveQuery.mockImplementation((sel: { type: string }) => {
        if (sel.type === 'label_assignment') return { data: [], loading: false };
        if (sel.type === 'label_rule') return { data: [fullRule], loading: false };
        return { data: [], loading: false };
      });

      renderLabels();
      // Switch to Rules tab
      const rulesTab = screen.getByText('Rules');
      rulesTab.click();

      expect(screen.getByText('Work documents')).toBeInTheDocument();
      expect(screen.getByText('/docs/')).toBeInTheDocument();
    });

    it('renders rules empty state', () => {
      mockUseLiveQuery.mockImplementation((sel: { type: string }) => {
        if (sel.type === 'label_rule') return { data: [], loading: false };
        return { data: [], loading: false };
      });

      renderLabels();
      screen.getByText('Rules').click();
      expect(screen.getByText('No label rules')).toBeInTheDocument();
    });

    it('does not crash with minimal rule (no name, no node_id)', () => {
      mockUseLiveQuery.mockImplementation((sel: { type: string }) => {
        if (sel.type === 'label_rule') return { data: [ruleMinimal], loading: false };
        return { data: [], loading: false };
      });

      renderLabels();
      screen.getByText('Rules').click();
      // Name should show "--" and Node should show "--"
      expect(screen.getAllByText('--').length).toBeGreaterThanOrEqual(2);
    });

    it('does not crash with undefined name and node_id', () => {
      mockUseLiveQuery.mockImplementation((sel: { type: string }) => {
        if (sel.type === 'label_rule') return { data: [ruleUndefinedName], loading: false };
        return { data: [], loading: false };
      });

      renderLabels();
      screen.getByText('Rules').click();
      expect(() => screen.getAllByText('--')).not.toThrow();
    });
  });
});
