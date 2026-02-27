import { useEffect, useState } from 'react';
import { BrowserRouter, Routes, Route, Navigate, Outlet } from 'react-router-dom';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { ThemeProvider } from 'next-themes';
import { AuthProvider, useAuth } from '@/contexts/AuthContext';
import { Toaster } from '@/components/ui/sonner';
import { Layout } from '@/components/Layout';
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

  useEffect(() => {
    fetch('/api/system/bootstrap-status')
      .then((r) => r.json())
      .then((data: { needs_bootstrap: boolean }) => {
        setBootstrapNeeded(data.needs_bootstrap);
      })
      .catch(() => {
        // If the check fails, assume no bootstrap needed and show login
        setBootstrapNeeded(false);
      });
  }, []);

  if (bootstrapNeeded === null) {
    // Still loading bootstrap status — render nothing to avoid flash
    return null;
  }

  return (
    <Routes>
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
        <Route element={<Layout />}>
          <Route path="/" element={<DashboardPage />} />
          <Route path="/files" element={<FileBrowserPage />} />
          <Route path="/search" element={<SearchPage />} />
          <Route path="/labels" element={<LabelsPage />} />
          <Route path="/vfs" element={<VfsPage />} />
          <Route path="/nodes" element={<NodesPage />} />
          <Route path="/nodes/:nodeId" element={<NodeDetailPage />} />
          <Route path="/storage" element={<StoragePage />} />
          <Route path="/settings" element={<SettingsPage />} />
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
