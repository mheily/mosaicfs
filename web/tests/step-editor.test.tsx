import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { StepEditor } from '@/components/StepEditor';

function makeStep(overrides = {}) {
  return {
    op: 'glob',
    value: '',
    comparator: '',
    invert: false,
    on_match: 'include' as const,
    ...overrides,
  };
}

function renderEditor(
  steps = [] as ReturnType<typeof makeStep>[],
  onChange = vi.fn(),
) {
  const user = userEvent.setup();
  const result = render(
    <MemoryRouter>
      <StepEditor steps={steps} onChange={onChange} />
    </MemoryRouter>,
  );
  return { ...result, onChange, user };
}

describe('StepEditor', () => {
  it('renders empty state with Add step button', () => {
    renderEditor();
    expect(screen.getByText('Add step')).toBeInTheDocument();
  });

  it('glob step renders text input with placeholder', () => {
    renderEditor([makeStep({ op: 'glob' })]);
    expect(screen.getByPlaceholderText('e.g. *.jpg')).toBeInTheDocument();
  });

  it('regex step renders text input with placeholder', () => {
    renderEditor([makeStep({ op: 'regex' })]);
    expect(screen.getByPlaceholderText(/data\/.*\\\.csv/)).toBeInTheDocument();
  });

  it('size step renders number input and unit selectors', () => {
    renderEditor([makeStep({ op: 'size' })]);
    expect(screen.getByPlaceholderText('0')).toBeInTheDocument();
  });

  it('age step renders number input and unit selectors', () => {
    renderEditor([makeStep({ op: 'age' })]);
    expect(screen.getByPlaceholderText('0')).toBeInTheDocument();
  });

  it('invert toggle calls onChange', async () => {
    const step = makeStep({ op: 'glob', invert: false });
    const onChange = vi.fn();
    const user = userEvent.setup();

    render(
      <MemoryRouter>
        <StepEditor steps={[step]} onChange={onChange} />
      </MemoryRouter>,
    );

    const invertSwitch = screen.getByRole('switch', { name: /invert/i });
    await user.click(invertSwitch);

    expect(onChange).toHaveBeenCalledWith([
      expect.objectContaining({ invert: true }),
    ]);
  });

  it('on_match selector renders current value', () => {
    const step = makeStep({ op: 'glob', on_match: 'include' });

    render(
      <MemoryRouter>
        <StepEditor steps={[step]} onChange={vi.fn()} />
      </MemoryRouter>,
    );

    // The on_match combobox should display "include"
    const triggers = screen.getAllByRole('combobox');
    const onMatchTrigger = triggers[triggers.length - 1];
    expect(onMatchTrigger).toHaveTextContent('include');
  });

  it('Add step adds a new step', async () => {
    const onChange = vi.fn();
    const user = userEvent.setup();

    render(
      <MemoryRouter>
        <StepEditor steps={[]} onChange={onChange} />
      </MemoryRouter>,
    );

    await user.click(screen.getByText('Add step'));

    expect(onChange).toHaveBeenCalledWith([
      expect.objectContaining({ op: 'glob', value: '', invert: false, on_match: 'include' }),
    ]);
  });

  it('Remove step removes it', async () => {
    const steps = [makeStep({ op: 'glob', value: 'a' }), makeStep({ op: 'regex', value: 'b' })];
    const onChange = vi.fn();
    const user = userEvent.setup();

    render(
      <MemoryRouter>
        <StepEditor steps={steps} onChange={onChange} />
      </MemoryRouter>,
    );

    // Find all remove buttons (X icons) - they're the last button in each step's toolbar
    const removeButtons = screen.getAllByRole('button').filter(
      (btn) => btn.querySelector('.lucide-x'),
    );
    await user.click(removeButtons[0]);

    expect(onChange).toHaveBeenCalledWith([
      expect.objectContaining({ op: 'regex', value: 'b' }),
    ]);
  });

  it('Move up/down reorders steps', async () => {
    const steps = [
      makeStep({ op: 'glob', value: 'first' }),
      makeStep({ op: 'regex', value: 'second' }),
    ];
    const onChange = vi.fn();
    const user = userEvent.setup();

    render(
      <MemoryRouter>
        <StepEditor steps={steps} onChange={onChange} />
      </MemoryRouter>,
    );

    // Find move down buttons (ArrowDown icons)
    const moveDownButtons = screen.getAllByRole('button').filter(
      (btn) => btn.querySelector('.lucide-arrow-down'),
    );
    // Click the first step's move down
    await user.click(moveDownButtons[0]);

    expect(onChange).toHaveBeenCalledWith([
      expect.objectContaining({ op: 'regex', value: 'second' }),
      expect.objectContaining({ op: 'glob', value: 'first' }),
    ]);
  });
});
