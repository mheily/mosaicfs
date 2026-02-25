import { Bell } from 'lucide-react';
import { useLiveQuery } from '@/hooks/useLiveQuery';
import { Button } from '@/components/ui/button';
import {
  Sheet,
  SheetContent,
  SheetTrigger,
} from '@/components/ui/sheet';
import { NotificationPanel } from './NotificationPanel';
import type { NotificationDoc } from './NotificationPanel';

export function NotificationBell() {
  const { data: notifications } = useLiveQuery<NotificationDoc>({
    type: 'notification',
  });

  const active = notifications.filter(
    (n) => n.status === 'active' || n.status === 'acknowledged',
  );
  const activeCount = active.filter((n) => n.status === 'active').length;
  const hasError = active.some((n) => n.severity === 'error');
  const hasWarning = active.some((n) => n.severity === 'warning');

  return (
    <Sheet>
      <SheetTrigger asChild>
        <Button variant="ghost" size="sm" className="relative">
          <Bell className="h-4 w-4" />
          {activeCount > 0 && (
            <span
              className={`absolute -top-0.5 -right-0.5 flex h-4 min-w-4 items-center justify-center rounded-full px-1 text-[10px] font-bold text-white ${
                hasError
                  ? 'bg-destructive'
                  : hasWarning
                    ? 'bg-amber-500'
                    : 'bg-blue-500'
              }`}
            >
              {activeCount}
            </span>
          )}
        </Button>
      </SheetTrigger>
      <SheetContent side="right" showCloseButton={false} className="p-0 w-80 sm:max-w-[360px]">
        <NotificationPanel notifications={notifications} />
      </SheetContent>
    </Sheet>
  );
}
