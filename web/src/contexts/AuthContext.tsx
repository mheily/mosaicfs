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
  expiresAt: string;
}

interface AuthContextValue {
  auth: AuthState | null;
  login: (accessKeyId: string, secretKey: string) => Promise<void>;
  logout: () => Promise<void>;
}

const AuthContext = createContext<AuthContextValue | null>(null);

export function AuthProvider({ children }: { children: ReactNode }) {
  const [auth, setAuth] = useState<AuthState | null>(null);

  const logout = useCallback(async () => {
    try {
      await api('/api/auth/logout', { method: 'POST' });
    } catch {
      // ignore
    }
    setAuthToken(null);
    await destroyDB();
    setAuth(null);
  }, []);

  useEffect(() => {
    setOnUnauthorized(() => {
      setAuthToken(null);
      destroyDB();
      setAuth(null);
    });
  }, []);

  const login = useCallback(async (accessKeyId: string, secretKey: string) => {
    const res = await api<{ token: string; expires_at: string }>(
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
    setAuth({
      token: res.token,
      accessKeyId,
      expiresAt: res.expires_at,
    });

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
