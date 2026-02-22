import { render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { FileDetailDrawer } from '@/components/FileDetailDrawer';

vi.mock('@/lib/api', () => ({
  api: vi.fn().mockResolvedValue([]),
}));

const mockFile = {
  _id: 'file-abc',
  path: '/data/photos/sunset.jpg',
  export_path: '/export/photos/sunset.jpg',
  node_id: 'node-1',
  size: 2048576,
  mime_type: 'image/jpeg',
  mtime: '2026-01-15T10:30:00Z',
  labels: ['photos', 'nature'],
};

function renderDrawer(file = mockFile) {
  return render(
    <MemoryRouter>
      <FileDetailDrawer file={file} onClose={vi.fn()} />
    </MemoryRouter>,
  );
}

describe('FileDetailDrawer', () => {
  it('renders file metadata (path, size, mime)', () => {
    renderDrawer();
    expect(screen.getByText('/data/photos/sunset.jpg')).toBeInTheDocument();
    expect(screen.getByText('image/jpeg')).toBeInTheDocument();
    // Size formatted
    expect(screen.getByText('2.0 MB')).toBeInTheDocument();
  });

  it('shows download button with correct link', () => {
    renderDrawer();
    const downloadLink = screen.getByRole('link', { name: /download/i });
    expect(downloadLink).toHaveAttribute('href', '/api/files/file-abc/content');
    expect(downloadLink).toHaveAttribute('download', 'sunset.jpg');
  });

  it('shows image preview for image/* mime types', () => {
    renderDrawer();
    const img = screen.getByRole('img', { name: 'sunset.jpg' });
    expect(img).toHaveAttribute('src', '/api/files/file-abc/content');
  });

  it('shows disabled Move, Rename, Delete buttons', () => {
    renderDrawer();
    const allButtons = screen.getAllByRole('button');
    const moveBtn = allButtons.find((b) => b.textContent?.includes('Move'));
    const renameBtn = allButtons.find((b) => b.textContent?.includes('Rename'));
    const deleteBtn = allButtons.find((b) => b.textContent?.includes('Delete'));

    expect(moveBtn).toBeDefined();
    expect(moveBtn).toBeDisabled();
    expect(renameBtn).toBeDisabled();
    expect(deleteBtn).toBeDisabled();
  });

  it('label chips are rendered', () => {
    renderDrawer();
    expect(screen.getByText('photos')).toBeInTheDocument();
    expect(screen.getByText('nature')).toBeInTheDocument();
  });
});
