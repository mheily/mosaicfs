import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import SearchPage from '@/pages/SearchPage';

const mockApi = vi.fn();

vi.mock('@/lib/api', () => ({
  api: (...args: unknown[]) => mockApi(...args),
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

describe('SearchPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    // Default: labels endpoint returns empty, search returns empty
    mockApi.mockImplementation(async (url: string) => {
      if (url === '/api/labels') return [];
      return [];
    });
  });

  it('renders search input', () => {
    renderSearch();
    expect(
      screen.getByPlaceholderText(/search files/i),
    ).toBeInTheDocument();
  });

  it('shows results after API response', async () => {
    mockApi.mockImplementation(async (url: string) => {
      if (url === '/api/labels') return [];
      if (typeof url === 'string' && url.startsWith('/api/search')) {
        return [
          { name: 'report.pdf', path: '/docs/report.pdf', node: 'node-1', size: 1024 },
        ];
      }
      return [];
    });

    const { user } = renderSearch();
    const input = screen.getByPlaceholderText(/search files/i);
    await user.type(input, 'report');

    await waitFor(() => {
      expect(screen.getByText('report.pdf')).toBeInTheDocument();
    });
  });

  it('debounces search calls (with mock returning immediately)', async () => {
    // Since useDebounce is mocked to return value immediately,
    // each keystroke triggers a search. Verify the API is called.
    mockApi.mockImplementation(async (url: string) => {
      if (url === '/api/labels') return [];
      return [];
    });

    const { user } = renderSearch();
    const input = screen.getByPlaceholderText(/search files/i);
    await user.type(input, 'abc');

    await waitFor(() => {
      // The search API should have been called (at least once per character due to mocked debounce)
      const searchCalls = mockApi.mock.calls.filter(
        (c) => typeof c[0] === 'string' && c[0].startsWith('/api/search'),
      );
      expect(searchCalls.length).toBeGreaterThan(0);
    });
  });

  it('label filter chips work', async () => {
    mockApi.mockImplementation(async (url: string) => {
      if (url === '/api/labels') return [{ name: 'photos' }, { name: 'docs' }];
      return [];
    });

    const { user } = renderSearch();

    await waitFor(() => {
      expect(screen.getByText('photos')).toBeInTheDocument();
    });

    // Click a label chip to activate it
    await user.click(screen.getByText('photos'));

    // Now type a query so search fires with the label
    const input = screen.getByPlaceholderText(/search files/i);
    await user.type(input, 'test');

    await waitFor(() => {
      const searchCalls = mockApi.mock.calls.filter(
        (c) => typeof c[0] === 'string' && c[0].startsWith('/api/search'),
      );
      const lastCall = searchCalls[searchCalls.length - 1];
      expect(lastCall[0]).toContain('labels=');
      expect(lastCall[0]).toContain('photos');
    });
  });
});
