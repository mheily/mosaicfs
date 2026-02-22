import { useNavigate } from 'react-router-dom';
import { cn } from '@/lib/utils';
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/components/ui/tooltip';

interface NodeBadgeProps {
  status: string;
  name?: string;
  nodeId?: string;
}

const statusStyles: Record<string, string> = {
  online: 'bg-green-100 text-green-800 dark:bg-green-900 dark:text-green-300',
  degraded: 'bg-amber-100 text-amber-800 dark:bg-amber-900 dark:text-amber-300',
  offline: 'bg-red-100 text-red-800 dark:bg-red-900 dark:text-red-300',
};

export function NodeBadge({ status, name, nodeId }: NodeBadgeProps) {
  const navigate = useNavigate();

  const label = name || status;
  const style = statusStyles[status] || statusStyles.offline;

  const pill = (
    <button
      onClick={nodeId ? () => navigate(`/nodes/${nodeId}`) : undefined}
      className={cn(
        'inline-flex items-center rounded-full px-2.5 py-0.5 text-xs font-medium transition-opacity hover:opacity-80',
        style,
        !nodeId && 'cursor-default',
      )}
    >
      {label}
    </button>
  );

  if (!nodeId) return pill;

  return (
    <TooltipProvider>
      <Tooltip>
        <TooltipTrigger asChild>{pill}</TooltipTrigger>
        <TooltipContent>
          <p>{nodeId} &mdash; {status}</p>
        </TooltipContent>
      </Tooltip>
    </TooltipProvider>
  );
}
