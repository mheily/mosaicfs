const STORE_NAME = 'settings.json';
const SERVER_URL_KEY = 'serverUrl';

// eslint-disable-next-line @typescript-eslint/no-explicit-any
let store: any = null;

async function getStore() {
  if (!store) {
    // Dynamic import — only resolves in Tauri runtime
    const { load } = await import('@tauri-apps/plugin-store' as string);
    store = await load(STORE_NAME, { autoSave: true });
  }
  return store;
}

export async function getServerUrl(): Promise<string | null> {
  const s = await getStore();
  return (await s.get(SERVER_URL_KEY)) ?? null;
}

export async function setServerUrl(url: string): Promise<void> {
  const s = await getStore();
  await s.set(SERVER_URL_KEY, url);
  await s.save();
}
