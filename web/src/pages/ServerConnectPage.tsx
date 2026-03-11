import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { setBaseUrl } from '@/lib/api';
import { setServerUrl } from '@/lib/tauri-store';

export default function ServerConnectPage() {
  const [url, setUrl] = useState('');
  const [error, setError] = useState('');
  const [connecting, setConnecting] = useState(false);
  const navigate = useNavigate();

  async function handleConnect(e: React.FormEvent) {
    e.preventDefault();
    setError('');
    setConnecting(true);

    const trimmed = url.replace(/\/+$/, '');
    if (!trimmed) {
      setError('Please enter a server URL');
      setConnecting(false);
      return;
    }

    try {
      const res = await fetch(`${trimmed}/api/system/bootstrap-status`);
      if (!res.ok) throw new Error(`Server returned ${res.status}`);
      await res.json();

      // Server is reachable — persist and apply
      await setServerUrl(trimmed);
      setBaseUrl(trimmed);
      navigate('/login', { replace: true });
    } catch (err) {
      setError(
        err instanceof Error
          ? `Could not connect: ${err.message}`
          : 'Could not connect to server',
      );
    } finally {
      setConnecting(false);
    }
  }

  return (
    <div className="flex h-screen items-center justify-center bg-background">
      {/* Drag region for macOS traffic lights */}
      <div
        data-tauri-drag-region
        className="fixed inset-x-0 top-0 h-7"
      />
      <div className="w-full max-w-sm rounded-lg border bg-card p-6 shadow-sm">
        <h1 className="mb-1 text-lg font-semibold">Connect to MosaicFS</h1>
        <p className="mb-4 text-sm text-muted-foreground">
          Enter the URL of your MosaicFS server
        </p>
        <form onSubmit={handleConnect}>
          <input
            type="url"
            value={url}
            onChange={(e) => setUrl(e.target.value)}
            placeholder="https://your-server:8443"
            className="mb-3 w-full rounded-md border bg-background px-3 py-2 text-sm outline-none focus:ring-2 focus:ring-ring"
            autoFocus
          />
          {error && (
            <p className="mb-3 text-sm text-destructive">{error}</p>
          )}
          <button
            type="submit"
            disabled={connecting}
            className="w-full rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground hover:bg-primary/90 disabled:opacity-50"
          >
            {connecting ? 'Connecting...' : 'Connect'}
          </button>
        </form>
      </div>
    </div>
  );
}
