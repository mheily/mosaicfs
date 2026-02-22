import { render, screen, act } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { renderHook } from '@testing-library/react';
import { AuthProvider, useAuth } from '@/contexts/AuthContext';

vi.mock('@/lib/api', () => ({
  api: vi.fn(),
  setAuthToken: vi.fn(),
  setOnUnauthorized: vi.fn(),
}));

vi.mock('@/lib/pouchdb', () => ({
  startSync: vi.fn(),
  destroyDB: vi.fn().mockResolvedValue(undefined),
}));

import { api, setAuthToken } from '@/lib/api';
import { startSync, destroyDB } from '@/lib/pouchdb';

function Wrapper({ children }: { children: React.ReactNode }) {
  return (
    <MemoryRouter>
      <AuthProvider>{children}</AuthProvider>
    </MemoryRouter>
  );
}

describe('AuthProvider', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders children', () => {
    render(<div data-testid="child">hello</div>, { wrapper: Wrapper });
    expect(screen.getByTestId('child')).toHaveTextContent('hello');
  });

  it('login calls API and sets token', async () => {
    vi.mocked(api).mockResolvedValueOnce({
      token: 'tok123',
      expires_at: '2026-12-31T00:00:00Z',
    });

    function LoginTest() {
      const { auth, login } = useAuth();
      return (
        <div>
          <button onClick={() => login('mykey', 'mysecret')}>Login</button>
          <span data-testid="token">{auth?.token ?? 'none'}</span>
        </div>
      );
    }

    render(<LoginTest />, { wrapper: Wrapper });
    expect(screen.getByTestId('token')).toHaveTextContent('none');

    await act(async () => {
      await userEvent.click(screen.getByText('Login'));
    });

    expect(api).toHaveBeenCalledWith('/api/auth/login', {
      method: 'POST',
      body: JSON.stringify({
        access_key_id: 'mykey',
        secret_key: 'mysecret',
      }),
    });
    expect(setAuthToken).toHaveBeenCalledWith('tok123');
    expect(startSync).toHaveBeenCalledWith('/db/mosaicfs');
    expect(screen.getByTestId('token')).toHaveTextContent('tok123');
  });

  it('logout clears auth state', async () => {
    vi.mocked(api)
      .mockResolvedValueOnce({ token: 'tok123', expires_at: '2026-12-31T00:00:00Z' })
      .mockResolvedValueOnce(undefined);

    function LogoutTest() {
      const { auth, login, logout } = useAuth();
      return (
        <div>
          <button onClick={() => login('k', 's')}>Login</button>
          <button onClick={() => logout()}>Logout</button>
          <span data-testid="token">{auth?.token ?? 'none'}</span>
        </div>
      );
    }

    render(<LogoutTest />, { wrapper: Wrapper });

    await act(async () => {
      await userEvent.click(screen.getByText('Login'));
    });
    expect(screen.getByTestId('token')).toHaveTextContent('tok123');

    await act(async () => {
      await userEvent.click(screen.getByText('Logout'));
    });

    expect(setAuthToken).toHaveBeenCalledWith(null);
    expect(destroyDB).toHaveBeenCalled();
    expect(screen.getByTestId('token')).toHaveTextContent('none');
  });

  it('useAuth throws outside provider', () => {
    expect(() => {
      renderHook(() => useAuth());
    }).toThrow('useAuth must be used within AuthProvider');
  });
});
