import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { LabelChip } from '@/components/LabelChip';

function renderChip(props: Parameters<typeof LabelChip>[0]) {
  return render(
    <MemoryRouter>
      <LabelChip {...props} />
    </MemoryRouter>,
  );
}

describe('LabelChip', () => {
  it('direct label renders with solid style (bg-primary)', () => {
    renderChip({ label: 'photos', inherited: false });
    const chip = screen.getByText('photos');
    expect(chip.closest('span')).toHaveClass('bg-primary');
  });

  it('inherited label renders with border style', () => {
    renderChip({ label: 'archive', inherited: true });
    const chip = screen.getByText('archive');
    expect(chip.closest('span')).toHaveClass('border');
  });

  it('remove button shown for direct labels with onRemove', () => {
    renderChip({ label: 'photos', inherited: false, onRemove: vi.fn() });
    expect(screen.getByRole('button', { name: /remove photos/i })).toBeInTheDocument();
  });

  it('remove button hidden for inherited labels', () => {
    renderChip({ label: 'archive', inherited: true, onRemove: vi.fn() });
    expect(screen.queryByRole('button', { name: /remove/i })).not.toBeInTheDocument();
  });

  it('onRemove callback fires when clicking X', async () => {
    const onRemove = vi.fn();
    const user = userEvent.setup();

    renderChip({ label: 'photos', inherited: false, onRemove });
    await user.click(screen.getByRole('button', { name: /remove photos/i }));

    expect(onRemove).toHaveBeenCalledTimes(1);
  });
});
