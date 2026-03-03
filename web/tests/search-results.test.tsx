import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import SearchPage from '@/pages/SearchPage';
import { fullSearchItem, searchItemMinimal, searchItemNullSource } from './fixtures';

const mockApi = vi.fn();

vi.mock('@/lib/api', () => ({
  api: (...args: unknown[]) => mockApi(...args),
  getAuthToken: vi.fn().mockReturnValue(null),
}));

vi.mock('@/hooks/useDebounce', () => ({
  useDebounce: (value: string) => value,
}));

function renderSearch() {
  const user = userEvent.setup();
  const result = render(
    <MemoryRouter>
      <SearchPage />
    </MemoryRouter>,
  );
  return { ...result, user };
}

describe('SearchPage boundary tests', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders results with all optional fields missing', async () => {
    mockApi.mockImplementation(async (url: string) => {
      if (url === '/api/labels') return { labels: [] };
      if (typeof url === 'string' && url.startsWith('/api/search')) {
        return { items: [searchItemMinimal], total: 1, offset: 0, limit: 50 };
      }
      return {};
    });

    const { user } = renderSearch();
    await user.type(screen.getByPlaceholderText(/search files/i), 'test');

    await waitFor(() => {
      expect(screen.getByText('unknown-file')).toBeInTheDocument();
    });
    // Size should show fallback
    expect(screen.getByText('--')).toBeInTheDocument();
  });

  it('renders results with null source without crash', async () => {
    mockApi.mockImplementation(async (url: string) => {
      if (url === '/api/labels') return { labels: [] };
      if (typeof url === 'string' && url.startsWith('/api/search')) {
        return { items: [searchItemNullSource], total: 1, offset: 0, limit: 50 };
      }
      return {};
    });

    const { user } = renderSearch();
    await user.type(screen.getByPlaceholderText(/search files/i), 'test');

    await waitFor(() => {
      expect(screen.getByText('no-source.txt')).toBeInTheDocument();
    });
  });

  it('renders full result items correctly', async () => {
    mockApi.mockImplementation(async (url: string) => {
      if (url === '/api/labels') return { labels: [] };
      if (typeof url === 'string' && url.startsWith('/api/search')) {
        return { items: [fullSearchItem], total: 1, offset: 0, limit: 50 };
      }
      return {};
    });

    const { user } = renderSearch();
    await user.type(screen.getByPlaceholderText(/search files/i), 'sunset');

    await waitFor(() => {
      expect(screen.getByText('sunset.jpg')).toBeInTheDocument();
      expect(screen.getByText('/photos/sunset.jpg')).toBeInTheDocument();
      expect(screen.getByText('node-laptop')).toBeInTheDocument();
    });
  });

  it('handles empty search results', async () => {
    mockApi.mockImplementation(async (url: string) => {
      if (url === '/api/labels') return { labels: [] };
      if (typeof url === 'string' && url.startsWith('/api/search')) {
        return { items: [], total: 0, offset: 0, limit: 50 };
      }
      return {};
    });

    const { user } = renderSearch();
    await user.type(screen.getByPlaceholderText(/search files/i), 'nonexistent');

    await waitFor(() => {
      expect(screen.getByText('No results found')).toBeInTheDocument();
    });
  });
});
