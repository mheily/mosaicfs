import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Switch } from '@/components/ui/switch';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { ArrowUp, ArrowDown, X, Plus } from 'lucide-react';

interface Step {
  op: string;
  value: string;
  comparator: string;
  invert: boolean;
  on_match: 'include' | 'exclude' | 'continue';
}

interface StepEditorProps {
  steps: Step[];
  onChange: (steps: Step[]) => void;
  readOnly?: boolean;
}

const OP_TYPES = [
  'glob',
  'regex',
  'age',
  'size',
  'mime',
  'node',
  'label',
  'access_age',
  'replicated',
  'annotation',
] as const;

const TIME_UNITS = ['minutes', 'hours', 'days'];
const SIZE_UNITS = ['B', 'KB', 'MB', 'GB'];

function defaultStep(): Step {
  return {
    op: 'glob',
    value: '',
    comparator: '',
    invert: false,
    on_match: 'include',
  };
}

function StepFields({
  step,
  onChange,
  readOnly,
}: {
  step: Step;
  onChange: (patch: Partial<Step>) => void;
  readOnly: boolean;
}) {
  switch (step.op) {
    case 'glob':
      return (
        <Input
          placeholder="e.g. *.jpg"
          value={step.value}
          onChange={(e) => onChange({ value: e.target.value })}
          disabled={readOnly}
        />
      );
    case 'regex':
      return (
        <Input
          placeholder="e.g. ^data/.*\\.csv$"
          value={step.value}
          onChange={(e) => onChange({ value: e.target.value })}
          disabled={readOnly}
        />
      );
    case 'age':
    case 'access_age': {
      const parts = step.value.split(' ');
      const num = parts[0] || '';
      const unit = parts[1] || 'days';
      return (
        <div className="flex items-center gap-2">
          <Select
            value={step.comparator || 'older_than'}
            onValueChange={(v) => onChange({ comparator: v })}
            disabled={readOnly}
          >
            <SelectTrigger className="w-[140px]">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="older_than">older than</SelectItem>
              <SelectItem value="newer_than">newer than</SelectItem>
            </SelectContent>
          </Select>
          <Input
            type="number"
            className="w-24"
            placeholder="0"
            value={num}
            onChange={(e) => onChange({ value: `${e.target.value} ${unit}` })}
            disabled={readOnly}
          />
          <Select
            value={unit}
            onValueChange={(v) => onChange({ value: `${num} ${v}` })}
            disabled={readOnly}
          >
            <SelectTrigger className="w-[110px]">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {TIME_UNITS.map((u) => (
                <SelectItem key={u} value={u}>
                  {u}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      );
    }
    case 'size': {
      const parts = step.value.split(' ');
      const num = parts[0] || '';
      const unit = parts[1] || 'MB';
      return (
        <div className="flex items-center gap-2">
          <Select
            value={step.comparator || 'larger_than'}
            onValueChange={(v) => onChange({ comparator: v })}
            disabled={readOnly}
          >
            <SelectTrigger className="w-[140px]">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="larger_than">larger than</SelectItem>
              <SelectItem value="smaller_than">smaller than</SelectItem>
            </SelectContent>
          </Select>
          <Input
            type="number"
            className="w-24"
            placeholder="0"
            value={num}
            onChange={(e) => onChange({ value: `${e.target.value} ${unit}` })}
            disabled={readOnly}
          />
          <Select
            value={unit}
            onValueChange={(v) => onChange({ value: `${num} ${v}` })}
            disabled={readOnly}
          >
            <SelectTrigger className="w-[90px]">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {SIZE_UNITS.map((u) => (
                <SelectItem key={u} value={u}>
                  {u}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      );
    }
    case 'mime':
      return (
        <Input
          placeholder="e.g. image/jpeg"
          value={step.value}
          onChange={(e) => onChange({ value: e.target.value })}
          disabled={readOnly}
        />
      );
    case 'node':
      return (
        <Input
          placeholder="Node ID"
          value={step.value}
          onChange={(e) => onChange({ value: e.target.value })}
          disabled={readOnly}
        />
      );
    case 'label':
      return (
        <Input
          placeholder="Label name"
          value={step.value}
          onChange={(e) => onChange({ value: e.target.value })}
          disabled={readOnly}
        />
      );
    case 'replicated':
      return (
        <div className="flex items-center gap-2">
          <Switch
            checked={step.value === 'true'}
            onCheckedChange={(checked) =>
              onChange({ value: String(checked) })
            }
            disabled={readOnly}
          />
          <span className="text-sm">
            {step.value === 'true' ? 'is replicated' : 'is not replicated'}
          </span>
        </div>
      );
    case 'annotation':
      return (
        <Input
          placeholder="Annotation key"
          value={step.value}
          onChange={(e) => onChange({ value: e.target.value })}
          disabled={readOnly}
        />
      );
    default:
      return null;
  }
}

export function StepEditor({
  steps,
  onChange,
  readOnly = false,
}: StepEditorProps) {
  const update = (index: number, patch: Partial<Step>) => {
    const next = steps.map((s, i) => (i === index ? { ...s, ...patch } : s));
    onChange(next);
  };

  const remove = (index: number) => {
    onChange(steps.filter((_, i) => i !== index));
  };

  const move = (index: number, dir: -1 | 1) => {
    const target = index + dir;
    if (target < 0 || target >= steps.length) return;
    const next = [...steps];
    [next[index], next[target]] = [next[target], next[index]];
    onChange(next);
  };

  const addStep = () => {
    onChange([...steps, defaultStep()]);
  };

  return (
    <div className="space-y-3">
      {steps.map((step, i) => (
        <div
          key={i}
          className="bg-card space-y-3 rounded-lg border p-4 shadow-sm"
        >
          <div className="flex items-center gap-2">
            <Select
              value={step.op}
              onValueChange={(v) => update(i, { op: v, value: '', comparator: '' })}
              disabled={readOnly}
            >
              <SelectTrigger className="w-[140px]">
                <SelectValue placeholder="Operation" />
              </SelectTrigger>
              <SelectContent>
                {OP_TYPES.map((op) => (
                  <SelectItem key={op} value={op}>
                    {op}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>

            {!readOnly && (
              <div className="ml-auto flex items-center gap-1">
                <Button
                  variant="ghost"
                  size="icon-xs"
                  onClick={() => move(i, -1)}
                  disabled={i === 0}
                >
                  <ArrowUp className="h-3 w-3" />
                </Button>
                <Button
                  variant="ghost"
                  size="icon-xs"
                  onClick={() => move(i, 1)}
                  disabled={i === steps.length - 1}
                >
                  <ArrowDown className="h-3 w-3" />
                </Button>
                <Button
                  variant="ghost"
                  size="icon-xs"
                  onClick={() => remove(i)}
                >
                  <X className="h-3 w-3" />
                </Button>
              </div>
            )}
          </div>

          <StepFields
            step={step}
            onChange={(patch) => update(i, patch)}
            readOnly={readOnly}
          />

          <div className="flex items-center gap-4">
            <div className="flex items-center gap-2">
              <Switch
                checked={step.invert}
                onCheckedChange={(checked) => update(i, { invert: checked })}
                disabled={readOnly}
                id={`invert-${i}`}
              />
              <label htmlFor={`invert-${i}`} className="text-sm">
                Invert
              </label>
            </div>

            <Select
              value={step.on_match}
              onValueChange={(v) =>
                update(i, { on_match: v as Step['on_match'] })
              }
              disabled={readOnly}
            >
              <SelectTrigger className="w-[120px]">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="include">include</SelectItem>
                <SelectItem value="exclude">exclude</SelectItem>
                <SelectItem value="continue">continue</SelectItem>
              </SelectContent>
            </Select>
          </div>
        </div>
      ))}

      {!readOnly && (
        <Button variant="outline" size="sm" onClick={addStep}>
          <Plus className="h-4 w-4" />
          Add step
        </Button>
      )}
    </div>
  );
}
