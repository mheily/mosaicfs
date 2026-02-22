import { NavLink } from 'react-router-dom';
import {
  LayoutDashboard,
  FolderOpen,
  Search,
  Tags,
  Network,
  Server,
  HardDrive,
  Settings,
} from 'lucide-react';
import { cn } from '@/lib/utils';

const navItems = [
  { to: '/', label: 'Dashboard', icon: LayoutDashboard },
  { to: '/files', label: 'File Browser', icon: FolderOpen },
  { to: '/search', label: 'Search', icon: Search },
  { to: '/labels', label: 'Labels', icon: Tags },
  { to: '/vfs', label: 'Virtual FS', icon: Network },
  { to: '/nodes', label: 'Nodes', icon: Server },
  { to: '/storage', label: 'Storage', icon: HardDrive },
  { to: '/settings', label: 'Settings', icon: Settings },
] as const;

export function Sidebar() {
  return (
    <>
      {/* Desktop sidebar */}
      <aside className="hidden md:flex md:flex-col md:w-56 md:border-r bg-muted/40 h-full">
        <div className="flex-1 overflow-y-auto py-4 px-3 space-y-1">
          {navItems.map(({ to, label, icon: Icon }) => (
            <NavLink
              key={to}
              to={to}
              end={to === '/'}
              className={({ isActive }) =>
                cn(
                  'flex items-center gap-3 rounded-md px-3 py-2 text-sm font-medium transition-colors',
                  isActive
                    ? 'bg-primary text-primary-foreground'
                    : 'text-muted-foreground hover:bg-accent hover:text-accent-foreground',
                )
              }
            >
              <Icon className="h-4 w-4 shrink-0" />
              {label}
            </NavLink>
          ))}
        </div>
      </aside>

      {/* Mobile bottom tab bar */}
      <nav className="fixed bottom-0 inset-x-0 z-50 flex md:hidden border-t bg-background">
        {navItems.map(({ to, label, icon: Icon }) => (
          <NavLink
            key={to}
            to={to}
            end={to === '/'}
            className={({ isActive }) =>
              cn(
                'flex flex-1 flex-col items-center gap-0.5 py-2 text-[10px] transition-colors',
                isActive
                  ? 'text-primary'
                  : 'text-muted-foreground',
              )
            }
          >
            <Icon className="h-5 w-5" />
            <span className="truncate">{label}</span>
          </NavLink>
        ))}
      </nav>
    </>
  );
}
