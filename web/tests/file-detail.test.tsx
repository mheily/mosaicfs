import { render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { FileDetailDrawer } from '@/components/FileDetailDrawer';
import {
  fullFileDetail,
  fileDetailMissingPath,
  fileDetailNullFields,
  fileDetailEmptyLabels,
  fileDetailZeroSize,
} from './fixtures';

vi.mock('@/lib/api', () => ({
  api: vi.fn().mockResolvedValue([]),
  getAuthToken: vi.fn().mockReturnValue(null),
}));

function renderDrawer(file = fullFileDetail) {
  return render(
    <MemoryRouter>
      <FileDetailDrawer file={file as any} onClose={vi.fn()} />
    </MemoryRouter>,
  );
}

describe('FileDetailDrawer', () => {
  it('renders file metadata (path, size, mime)', () => {
    renderDrawer();
    expect(screen.getByText('/photos/sunset.jpg')).toBeInTheDocument();
    expect(screen.getByText('image/jpeg')).toBeInTheDocument();
    expect(screen.getByText('2.0 MB')).toBeInTheDocument();
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

  it('renders label chips', () => {
    renderDrawer();
    expect(screen.getByText('photos')).toBeInTheDocument();
    expect(screen.getByText('nature')).toBeInTheDocument();
  });

  // ── Boundary tests ──

  it('does not crash when path is undefined', () => {
    expect(() => renderDrawer(fileDetailMissingPath)).not.toThrow();
    // Should show fallback for the filename
    expect(screen.getByText('(unknown)')).toBeInTheDocument();
    // Path field should show dash
    expect(screen.getAllByText('—').length).toBeGreaterThan(0);
  });

  it('does not crash when multiple fields are null', () => {
    expect(() => renderDrawer(fileDetailNullFields)).not.toThrow();
  });

  it('renders with empty labels array', () => {
    expect(() => renderDrawer(fileDetailEmptyLabels)).not.toThrow();
  });

  it('renders size 0 correctly', () => {
    renderDrawer(fileDetailZeroSize);
    expect(screen.getByText('0 B')).toBeInTheDocument();
  });

  it('renders null file without crash', () => {
    render(
      <MemoryRouter>
        <FileDetailDrawer file={null} onClose={vi.fn()} />
      </MemoryRouter>,
    );
    // When file is null, the sheet should not render file content
    expect(screen.queryByText('Metadata')).not.toBeInTheDocument();
  });
});
