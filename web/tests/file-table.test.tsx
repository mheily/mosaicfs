import { render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { FileTable } from '@/components/FileTable';
import { fullFileEntry, fileEntryUndefinedSource, fileEntryUndefinedSize } from './fixtures';

function renderTable(files = [fullFileEntry]) {
  return render(
    <MemoryRouter>
      <FileTable
        files={files}
        onFileClick={vi.fn()}
        sortField="name"
        sortDir="asc"
        onSort={vi.fn()}
      />
    </MemoryRouter>,
  );
}

describe('FileTable', () => {
  it('renders full file entries', () => {
    renderTable();
    expect(screen.getByText('report.pdf')).toBeInTheDocument();
  });

  it('renders "No files found" for empty list', () => {
    renderTable([]);
    expect(screen.getByText('No files found')).toBeInTheDocument();
  });

  it('does not crash when source is undefined', () => {
    expect(() => renderTable([fileEntryUndefinedSource as any])).not.toThrow();
    expect(screen.getByText('orphan.txt')).toBeInTheDocument();
    // Should show a fallback for the node column
    expect(screen.getByText('—')).toBeInTheDocument();
  });

  it('does not crash when size is undefined', () => {
    expect(() => renderTable([fileEntryUndefinedSize as any])).not.toThrow();
    expect(screen.getByText('—')).toBeInTheDocument();
  });
});
