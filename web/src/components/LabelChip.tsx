import { X } from 'lucide-react';
import { cn } from '@/lib/utils';
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/components/ui/tooltip';

interface LabelChipProps {
  label: string;
  inherited: boolean;
  onRemove?: () => void;
}

export function LabelChip({ label, inherited, onRemove }: LabelChipProps) {
  const chip = (
    <span
      className={cn(
        'inline-flex items-center gap-1 rounded-full px-2.5 py-0.5 text-xs font-medium',
        inherited
          ? 'border border-muted-foreground/40 text-muted-foreground'
          : 'bg-primary text-primary-foreground',
      )}
    >
      {label}
      {onRemove && !inherited && (
        <button
          onClick={onRemove}
          className="ml-0.5 rounded-full p-0.5 hover:bg-black/10 dark:hover:bg-white/10"
          aria-label={`Remove ${label}`}
        >
          <X className="h-3 w-3" />
        </button>
      )}
    </span>
  );

  if (!inherited) return chip;

  return (
    <TooltipProvider>
      <Tooltip>
        <TooltipTrigger asChild>{chip}</TooltipTrigger>
        <TooltipContent>Inherited from parent</TooltipContent>
      </Tooltip>
    </TooltipProvider>
  );
}
