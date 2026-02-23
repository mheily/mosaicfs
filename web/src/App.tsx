import { BrowserRouter, Routes, Route, Navigate, Outlet } from 'react-router-dom';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { ThemeProvider } from 'next-themes';
import { AuthProvider, useAuth } from '@/contexts/AuthContext';
import { Toaster } from '@/components/ui/sonner';
import { Layout } from '@/components/Layout';
import LoginPage from '@/pages/LoginPage';
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

function AuthGuard() {
  const { auth } = useAuth();
  if (!auth) return <Navigate to="/login" replace />;
  return <Outlet />;
}

function App() {
  return (
    <ThemeProvider attribute="class" defaultTheme="system" enableSystem>
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>
        <AuthProvider>
          <Routes>
            <Route path="/login" element={<LoginPage />} />
            <Route element={<AuthGuard />}>
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
          <Toaster />
        </AuthProvider>
      </BrowserRouter>
    </QueryClientProvider>
    </ThemeProvider>
  );
}

export default App;
