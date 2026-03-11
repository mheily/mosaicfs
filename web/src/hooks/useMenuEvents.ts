import { useEffect } from 'react';
import { useNavigate } from 'react-router-dom';
import { isTauri } from '@/lib/platform';

export function useMenuEvents() {
  const navigate = useNavigate();

  useEffect(() => {
    if (!isTauri()) return;

    let unlisten: (() => void) | undefined;

    (async () => {
      const { listen } = await import('@tauri-apps/api/event' as string);
      unlisten = await listen('menu-action', (event: { payload: string }) => {
        switch (event.payload) {
          case 'go-back':
            navigate(-1);
            break;
          case 'go-forward':
            navigate(1);
            break;
          case 'go-enclosing': {
            // Navigate to parent directory
            const params = new URLSearchParams(window.location.search);
            const currentPath = params.get('path') || '/';
            if (currentPath !== '/') {
              const parts = currentPath.split('/').filter(Boolean);
              parts.pop();
              const parent = parts.length > 0 ? '/' + parts.join('/') : '/';
              navigate(`/files?path=${encodeURIComponent(parent)}`);
            }
            break;
          }
        }
      });
    })();

    return () => {
      unlisten?.();
    };
  }, [navigate]);
}
