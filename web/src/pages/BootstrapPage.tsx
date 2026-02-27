import { useState, type FormEvent } from 'react';
import { useNavigate } from 'react-router-dom';
import { useAuth } from '@/contexts/AuthContext';
import { KeyRound, LogIn, Copy, Check } from 'lucide-react';
import { api } from '@/lib/api';

type Step = 'token' | 'credentials';

interface GeneratedCredential {
  access_key_id: string;
  secret_key: string;
}

function CopyButton({ value }: { value: string }) {
  const [copied, setCopied] = useState(false);

  async function handleCopy() {
    await navigator.clipboard.writeText(value);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  }

  return (
    <button
      type="button"
      onClick={handleCopy}
      className="ml-2 shrink-0 rounded p-1 text-muted-foreground hover:text-foreground"
      title="Copy to clipboard"
    >
      {copied ? <Check className="h-3.5 w-3.5 text-green-500" /> : <Copy className="h-3.5 w-3.5" />}
    </button>
  );
}

export default function BootstrapPage({ onComplete }: { onComplete: () => void }) {
  const { login } = useAuth();
  const navigate = useNavigate();

  const [step, setStep] = useState<Step>('token');
  const [token, setToken] = useState('');
  const [credential, setCredential] = useState<GeneratedCredential | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  async function handleTokenSubmit(e: FormEvent) {
    e.preventDefault();
    setError(null);
    setLoading(true);
    try {
      const result = await api<GeneratedCredential>('/api/system/bootstrap', {
        method: 'POST',
        body: JSON.stringify({ token: token.trim() }),
      });
      setCredential(result);
      setStep('credentials');
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : 'Setup failed');
    } finally {
      setLoading(false);
    }
  }

  async function handleSignIn() {
    if (!credential) return;
    setLoading(true);
    try {
      onComplete();
      await login(credential.access_key_id, credential.secret_key);
      navigate('/');
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : 'Login failed');
      setLoading(false);
    }
  }

  return (
    <div className="flex min-h-screen items-center justify-center bg-muted/40 p-4">
      <div className="w-full max-w-sm rounded-lg border bg-card p-6 shadow-sm">
        <div className="mb-6 text-center">
          <h1 className="text-2xl font-bold">MosaicFS</h1>
          <p className="text-sm text-muted-foreground">First-time setup</p>
        </div>

        {error && (
          <div className="mb-4 rounded-md bg-destructive/10 p-3 text-sm text-destructive">
            {error}
          </div>
        )}

        {step === 'token' && (
          <form onSubmit={handleTokenSubmit} className="space-y-4">
            <p className="text-sm text-muted-foreground">
              Enter the bootstrap token. You can find it in the server logs — look
              for a line like{' '}
              <span className="font-mono text-foreground">
                the bootstrap token is &lt;token&gt;
              </span>
              .
            </p>

            <div className="space-y-2">
              <label htmlFor="bootstrap-token" className="text-sm font-medium leading-none">
                Bootstrap Token
              </label>
              <input
                id="bootstrap-token"
                type="text"
                value={token}
                onChange={(e) => setToken(e.target.value)}
                required
                autoComplete="off"
                className="flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 font-mono text-sm shadow-sm transition-colors placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                placeholder="xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"
              />
            </div>

            <button
              type="submit"
              disabled={loading}
              className="inline-flex h-9 w-full items-center justify-center gap-2 rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground shadow hover:bg-primary/90 disabled:pointer-events-none disabled:opacity-50"
            >
              <KeyRound className="h-4 w-4" />
              {loading ? 'Verifying...' : 'Continue'}
            </button>
          </form>
        )}

        {step === 'credentials' && credential && (
          <div className="space-y-4">
            <div className="rounded-md border border-amber-200 bg-amber-50 p-3 text-sm text-amber-800 dark:border-amber-800 dark:bg-amber-950 dark:text-amber-200">
              Save these credentials now. The secret key will{' '}
              <strong>never be shown again</strong>.
            </div>

            <div className="space-y-3">
              <div className="space-y-1">
                <p className="text-xs font-medium text-muted-foreground">Access Key ID</p>
                <div className="flex items-center rounded-md border bg-muted/50 px-3 py-2">
                  <span className="flex-1 font-mono text-xs break-all">
                    {credential.access_key_id}
                  </span>
                  <CopyButton value={credential.access_key_id} />
                </div>
              </div>

              <div className="space-y-1">
                <p className="text-xs font-medium text-muted-foreground">Secret Key</p>
                <div className="flex items-center rounded-md border bg-muted/50 px-3 py-2">
                  <span className="flex-1 font-mono text-xs break-all">
                    {credential.secret_key}
                  </span>
                  <CopyButton value={credential.secret_key} />
                </div>
              </div>
            </div>

            <button
              type="button"
              onClick={handleSignIn}
              disabled={loading}
              className="inline-flex h-9 w-full items-center justify-center gap-2 rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground shadow hover:bg-primary/90 disabled:pointer-events-none disabled:opacity-50"
            >
              <LogIn className="h-4 w-4" />
              {loading ? 'Signing in...' : 'Sign In'}
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
