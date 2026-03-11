import { useEffect, useState } from 'react';
import { BrowserRouter, Routes, Route, Navigate, Outlet } from 'react-router-dom';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { ThemeProvider } from 'next-themes';
import { AuthProvider, useAuth } from '@/contexts/AuthContext';
import { api, setBaseUrl, getBaseUrl } from '@/lib/api';
import { isTauri } from '@/lib/platform';
import { Toaster } from '@/components/ui/sonner';
import { Layout } from '@/components/Layout';
import { FinderLayout } from '@/components/FinderLayout';
import LoginPage from '@/pages/LoginPage';
import BootstrapPage from '@/pages/BootstrapPage';
import DashboardPage from '@/pages/DashboardPage';
import FileBrowserPage from '@/pages/FileBrowserPage';
import SearchPage from '@/pages/SearchPage';
import LabelsPage from '@/pages/LabelsPage';
import VfsPage from '@/pages/VfsPage';
import NodesPage from '@/pages/NodesPage';
import NodeDetailPage from '@/pages/NodeDetailPage';
import StoragePage from '@/pages/StoragePage';
import SettingsPage from '@/pages/SettingsPage';
import DbConsolePage from '@/pages/DbConsolePage';
import ServerConnectPage from '@/pages/ServerConnectPage';

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      retry: 1,
      refetchOnWindowFocus: false,
    },
  },
});

function AuthGuard({ bootstrapNeeded }: { bootstrapNeeded: boolean }) {
  const { auth } = useAuth();
  if (bootstrapNeeded) return <Navigate to="/setup" replace />;
  if (!auth) return <Navigate to="/login" replace />;
  return <Outlet />;
}

function AppRoutes() {
  const [bootstrapNeeded, setBootstrapNeeded] = useState<boolean | null>(null);
  const [tauriReady, setTauriReady] = useState(!isTauri());

  // In Tauri mode, load the stored server URL before anything else
  useEffect(() => {
    if (!isTauri()) return;
    (async () => {
      try {
        const { getServerUrl } = await import('@/lib/tauri-store');
        const url = await getServerUrl();
        if (url) {
          setBaseUrl(url);
        }
      } catch {
        // Store not available — will redirect to /connect
      }
      setTauriReady(true);
    })();
  }, []);

  useEffect(() => {
    if (!tauriReady) return;
    api<{ needs_bootstrap: boolean }>('/api/system/bootstrap-status')
      .then((data) => {
        setBootstrapNeeded(data.needs_bootstrap);
      })
      .catch(() => {
        // If the check fails, assume no bootstrap needed and show login
        setBootstrapNeeded(false);
      });
  }, [tauriReady]);

  if (!tauriReady || bootstrapNeeded === null) {
    return null;
  }

  const tauri = isTauri();
  const FileLayout = tauri ? FinderLayout : Layout;

  // In Tauri mode, if no server URL is configured, redirect to /connect
  if (tauri && !getBaseUrl()) {
    return (
      <Routes>
        <Route path="/connect" element={<ServerConnectPage />} />
        <Route path="*" element={<Navigate to="/connect" replace />} />
      </Routes>
    );
  }

  return (
    <Routes>
      {/* Tauri-only: server connect page */}
      {tauri && (
        <Route path="/connect" element={<ServerConnectPage />} />
      )}
      <Route
        path="/setup"
        element={
          bootstrapNeeded
            ? <BootstrapPage onComplete={() => setBootstrapNeeded(false)} />
            : <Navigate to="/login" replace />
        }
      />
      <Route
        path="/login"
        element={bootstrapNeeded ? <Navigate to="/setup" replace /> : <LoginPage />}
      />
      <Route element={<AuthGuard bootstrapNeeded={bootstrapNeeded} />}>
        {/* In Tauri mode, /files uses FinderLayout; everything else uses standard Layout */}
        <Route element={<FileLayout />}>
          <Route path="/files" element={<FileBrowserPage />} />
        </Route>
        <Route element={<Layout />}>
          <Route path="/" element={<DashboardPage />} />
          <Route path="/search" element={<SearchPage />} />
          <Route path="/labels" element={<LabelsPage />} />
          <Route path="/vfs" element={<VfsPage />} />
          <Route path="/nodes" element={<NodesPage />} />
          <Route path="/nodes/:nodeId" element={<NodeDetailPage />} />
          <Route path="/storage" element={<StoragePage />} />
          <Route path="/settings" element={<SettingsPage />} />
          <Route path="/db-console" element={<DbConsolePage />} />
        </Route>
      </Route>
    </Routes>
  );
}

function App() {
  return (
    <ThemeProvider attribute="class" defaultTheme="system" enableSystem>
      <QueryClientProvider client={queryClient}>
        <BrowserRouter>
          <AuthProvider>
            <AppRoutes />
            <Toaster />
          </AuthProvider>
        </BrowserRouter>
      </QueryClientProvider>
    </ThemeProvider>
  );
}

export default App;
