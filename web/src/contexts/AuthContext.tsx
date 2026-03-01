import {
  createContext,
  useContext,
  useState,
  useCallback,
  useEffect,
  type ReactNode,
} from 'react';
import { api, setAuthToken, setOnUnauthorized } from '@/lib/api';
import { startSync, destroyDB } from '@/lib/pouchdb';

interface AuthState {
  token: string;
  accessKeyId: string;
  name?: string;
  expiresAt: string;
}

interface AuthContextValue {
  auth: AuthState | null;
  login: (accessKeyId: string, secretKey: string) => Promise<void>;
  logout: () => Promise<void>;
}

const AuthContext = createContext<AuthContextValue | null>(null);

const AUTH_STORAGE_KEY = 'mosaicfs_auth';

function saveAuth(state: AuthState) {
  localStorage.setItem(AUTH_STORAGE_KEY, JSON.stringify(state));
}

function clearAuth() {
  localStorage.removeItem(AUTH_STORAGE_KEY);
}

function loadAuth(): AuthState | null {
  try {
    const raw = localStorage.getItem(AUTH_STORAGE_KEY);
    if (!raw) return null;
    const state: AuthState = JSON.parse(raw);
    if (new Date(state.expiresAt) <= new Date()) {
      clearAuth();
      return null;
    }
    return state;
  } catch {
    clearAuth();
    return null;
  }
}

export function AuthProvider({ children }: { children: ReactNode }) {
  const [auth, setAuth] = useState<AuthState | null>(() => {
    const stored = loadAuth();
    if (stored) {
      setAuthToken(stored.token);
    }
    return stored;
  });

  // Start PouchDB sync if a session was restored from storage
  useEffect(() => {
    if (auth) {
      startSync('/db/mosaicfs');
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Backfill name for sessions restored from storage that predate the name field
  useEffect(() => {
    if (auth && !auth.name) {
      api<{ name: string }>(`/api/credentials/${auth.accessKeyId}`)
        .then((cred) => {
          const updated = { ...auth, name: cred.name };
          saveAuth(updated);
          setAuth(updated);
        })
        .catch(() => {/* ignore – name stays blank */});
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const logout = useCallback(async () => {
    try {
      await api('/api/auth/logout', { method: 'POST' });
    } catch {
      // ignore
    }
    setAuthToken(null);
    clearAuth();
    await destroyDB();
    setAuth(null);
  }, []);

  useEffect(() => {
    setOnUnauthorized(() => {
      setAuthToken(null);
      clearAuth();
      destroyDB();
      setAuth(null);
    });
  }, []);

  const login = useCallback(async (accessKeyId: string, secretKey: string) => {
    const res = await api<{ token: string; expires_at: number }>(
      '/api/auth/login',
      {
        method: 'POST',
        body: JSON.stringify({
          access_key_id: accessKeyId,
          secret_key: secretKey,
        }),
      },
    );

    setAuthToken(res.token);

    const cred = await api<{ name: string }>(`/api/credentials/${accessKeyId}`);

    const state: AuthState = {
      token: res.token,
      accessKeyId,
      name: cred.name,
      expiresAt: new Date(res.expires_at * 1000).toISOString(),
    };
    saveAuth(state);
    setAuth(state);

    // Start PouchDB sync
    startSync('/db/mosaicfs');
  }, []);

  return (
    <AuthContext.Provider value={{ auth, login, logout }}>
      {children}
    </AuthContext.Provider>
  );
}

export function useAuth() {
  const ctx = useContext(AuthContext);
  if (!ctx) throw new Error('useAuth must be used within AuthProvider');
  return ctx;
}
