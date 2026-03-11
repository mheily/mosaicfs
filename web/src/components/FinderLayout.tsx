import { Outlet, useNavigate, useLocation } from 'react-router-dom';
import { ChevronLeft, User } from 'lucide-react';
import { useAuth } from '@/contexts/AuthContext';
import { isTauri } from '@/lib/platform';
import { useMenuEvents } from '@/hooks/useMenuEvents';

export function FinderLayout() {
  const navigate = useNavigate();
  const location = useLocation();
  const { auth, logout } = useAuth();

  useMenuEvents();

  // Build breadcrumb from current path search params
  const params = new URLSearchParams(location.search);
  const vfsPath = params.get('path') || '/';
  const segments = vfsPath === '/' ? [] : vfsPath.split('/').filter(Boolean);

  return (
    <div className="flex h-screen flex-col overflow-hidden bg-background">
      {/* macOS traffic-light drag region */}
      {isTauri() && (
        <div
          data-tauri-drag-region
          className="h-7 shrink-0"
        />
      )}

      {/* Compact toolbar */}
      <div className="flex h-10 shrink-0 items-center gap-2 border-b px-3">
        <button
          onClick={() => navigate(-1)}
          className="rounded p-1 hover:bg-accent"
          title="Back"
        >
          <ChevronLeft className="h-4 w-4" />
        </button>

        {/* Breadcrumb path bar */}
        <nav className="flex min-w-0 flex-1 items-center gap-1 text-xs text-muted-foreground">
          <button
            onClick={() => navigate('/files?path=/')}
            className="shrink-0 hover:text-foreground"
          >
            root
          </button>
          {segments.map((seg, i) => {
            const path = '/' + segments.slice(0, i + 1).join('/');
            return (
              <span key={path} className="flex items-center gap-1">
                <span>/</span>
                <button
                  onClick={() => navigate(`/files?path=${encodeURIComponent(path)}`)}
                  className="truncate hover:text-foreground"
                >
                  {seg}
                </button>
              </span>
            );
          })}
        </nav>

        {/* User menu (minimal) */}
        {auth && (
          <button
            onClick={logout}
            className="flex items-center gap-1 rounded px-2 py-1 text-xs text-muted-foreground hover:bg-accent hover:text-foreground"
            title={`Logged in as ${auth.name || auth.accessKeyId}`}
          >
            <User className="h-3.5 w-3.5" />
          </button>
        )}
      </div>

      {/* Content */}
      <main className="flex-1 overflow-hidden">
        <Outlet />
      </main>
    </div>
  );
}
