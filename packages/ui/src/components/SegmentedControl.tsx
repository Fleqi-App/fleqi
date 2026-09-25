import { ToggleGroup, ToggleGroupItem } from "./ui/toggle-group";

export interface SegmentedOption<T extends string> { value: T; label: string }
export interface SegmentedControlProps<T extends string> {
  id: string; value: T; options: ReadonlyArray<SegmentedOption<T>>;
  disabled?: boolean; onChange: (value: T) => void;
  "aria-label"?: string; "aria-describedby"?: string;
}

/** Official Radix/shadcn toggle primitives with single-choice keyboard semantics. */
export function SegmentedControl<T extends string>({ id, value, options, disabled, onChange, ...rest }: SegmentedControlProps<T>) {
  return <ToggleGroup type="single" id={id} role="radiogroup" value={value} disabled={disabled} data-slot="segmented" data-value={value} className="gap-0 rounded-lg border border-border bg-muted p-0.5" onValueChange={(next) => { const option = options.find((item) => item.value === next); if (option) onChange(option.value); }} {...rest}>
    {options.map((option, index) => <ToggleGroupItem key={option.value} value={option.value} role="radio" aria-checked={value === option.value} data-value={option.value} className="h-7 min-w-0 rounded-md px-2.5 text-xs data-[state=on]:bg-card data-[state=on]:text-foreground data-[state=on]:shadow-sm" onKeyDown={(event) => {
      const delta = ["ArrowRight", "ArrowDown"].includes(event.key) ? 1 : ["ArrowLeft", "ArrowUp"].includes(event.key) ? -1 : 0;
      if (!delta) return;
      event.preventDefault();
      const next = (index + delta + options.length) % options.length;
      const target = options[next];
      if (target) { onChange(target.value); event.currentTarget.parentElement?.querySelectorAll<HTMLButtonElement>("button[role=radio]")[next]?.focus(); }
    }}>{option.label}</ToggleGroupItem>)}
  </ToggleGroup>;
}
