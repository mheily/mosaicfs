import { renderHook, act, waitFor } from '@testing-library/react';
import { useLiveQuery } from '@/hooks/useLiveQuery';
import { useLiveDoc } from '@/hooks/useLiveDoc';

type ChangeListener = () => void;

function createMockDB(docs: Record<string, unknown> = {}) {
  let changeListeners: ChangeListener[] = [];

  return {
    find: vi.fn().mockImplementation(async () => ({
      docs: Object.values(docs),
    })),
    get: vi.fn().mockImplementation(async (id: string) => {
      if (docs[id]) return docs[id];
      throw { status: 404, message: 'missing' };
    }),
    changes: vi.fn().mockReturnValue({
      on(event: string, fn: ChangeListener) {
        if (event === 'change') changeListeners.push(fn);
        return this;
      },
      cancel: vi.fn(),
    }),
    _fireChange() {
      changeListeners.forEach((fn) => fn());
    },
    _setDocs(newDocs: Record<string, unknown>) {
      Object.assign(docs, newDocs);
    },
    _resetListeners() {
      changeListeners = [];
    },
  };
}

let mockDB: ReturnType<typeof createMockDB>;

vi.mock('@/lib/pouchdb', () => ({
  getDB: () => mockDB,
}));

describe('useLiveQuery', () => {
  beforeEach(() => {
    mockDB = createMockDB({
      doc1: { _id: 'doc1', type: 'file', name: 'a.txt' },
      doc2: { _id: 'doc2', type: 'file', name: 'b.txt' },
    });
  });

  it('returns data from PouchDB find', async () => {
    const { result } = renderHook(() =>
      useLiveQuery({ type: 'file' }),
    );

    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.data).toHaveLength(2);
    expect(mockDB.find).toHaveBeenCalled();
  });

  it('updates when changes fire', async () => {
    const { result } = renderHook(() =>
      useLiveQuery({ type: 'file' }),
    );

    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.data).toHaveLength(2);

    // Add a doc and fire change
    mockDB._setDocs({ doc3: { _id: 'doc3', type: 'file', name: 'c.txt' } });
    mockDB.find.mockResolvedValueOnce({
      docs: [
        { _id: 'doc1', type: 'file', name: 'a.txt' },
        { _id: 'doc2', type: 'file', name: 'b.txt' },
        { _id: 'doc3', type: 'file', name: 'c.txt' },
      ],
    });

    act(() => {
      mockDB._fireChange();
    });

    await waitFor(() => expect(result.current.data).toHaveLength(3));
  });
});

describe('useLiveDoc', () => {
  beforeEach(() => {
    mockDB = createMockDB({
      doc1: { _id: 'doc1', type: 'file', name: 'a.txt' },
    });
  });

  it('returns a single document', async () => {
    const { result } = renderHook(() => useLiveDoc('doc1'));

    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.doc).toEqual({ _id: 'doc1', type: 'file', name: 'a.txt' });
  });

  it('returns null for missing doc', async () => {
    const { result } = renderHook(() => useLiveDoc('nonexistent'));

    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.doc).toBeNull();
  });
});
