import { render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { NotificationPanel } from '@/components/NotificationPanel';
import {
  fullNotification,
  notificationMinimal,
  notificationUndefinedActions,
  notificationNullSource,
} from './fixtures';

function renderPanel(notifications = [fullNotification]) {
  return render(
    <MemoryRouter>
      <NotificationPanel notifications={notifications as any[]} />
    </MemoryRouter>,
  );
}

describe('NotificationPanel', () => {
  it('renders notifications with full data', () => {
    renderPanel();
    expect(screen.getByText('Crawl complete')).toBeInTheDocument();
    expect(screen.getByText('Initial crawl finished successfully.')).toBeInTheDocument();
  });

  it('renders "No notifications" for empty list', () => {
    renderPanel([]);
    expect(screen.getByText('No notifications')).toBeInTheDocument();
  });

  it('does not crash with undefined actions', () => {
    expect(() => renderPanel([notificationUndefinedActions as any])).not.toThrow();
    expect(screen.getByText('Crawl complete')).toBeInTheDocument();
  });

  it('does not crash with minimal notification (no actions/ack/resolve)', () => {
    expect(() => renderPanel([notificationMinimal])).not.toThrow();
    expect(screen.getByText('Test warning')).toBeInTheDocument();
  });

  it('does not crash when source is null', () => {
    expect(() => renderPanel([notificationNullSource as any])).not.toThrow();
  });

  it('renders action buttons when present', () => {
    renderPanel([fullNotification]);
    expect(screen.getByText('View Files')).toBeInTheDocument();
  });

  it('renders occurrence count badge when > 1', () => {
    const multiOccurrence = { ...fullNotification, occurrence_count: 5 };
    renderPanel([multiOccurrence]);
    expect(screen.getByText('x5')).toBeInTheDocument();
  });
});
