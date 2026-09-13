import { cn } from "@/lib/utils";
import type { LucideIcon } from "lucide-react";

/**
 * The underline tab row used by the Extensions page and the profile editor.
 *
 * Both had it written out one `<button>` per tab with the same long className
 * repeated verbatim — four copies each. That is how `py-0.2` survived in two of
 * the profile editor's tabs: an invalid Tailwind step in a string nobody diffs
 * against its three siblings.
 */
export interface TabItem<T extends string> {
  id: T;
  label: string;
  icon?: LucideIcon;
  /** Rendered as a pill after the label. `0` still shows; use `undefined` to omit. */
  count?: number;
}

interface TabsProps<T extends string> {
  items: readonly TabItem<T>[];
  value: T;
  onChange: (id: T) => void;
  className?: string;
}

/*
 * Deliberately plain buttons, not `role="tab"`.
 *
 * The panels these switch between are not marked up as `role="tabpanel"` and
 * there is no arrow-key navigation, so claiming the tab pattern would have a
 * screen reader announce a widget whose contract this does not honour. A pressed
 * button is the honest description of what these are.
 */
export function Tabs<T extends string>({ items, value, onChange, className }: TabsProps<T>) {
  return (
    <div className={cn("flex items-center gap-1 border-b", className)}>
      {items.map(({ id, label, icon: Icon, count }) => {
        const active = id === value;
        return (
          <button
            key={id}
            type="button"
            aria-pressed={active}
            onClick={() => onChange(id)}
            className={cn(
              "group flex items-center gap-2 px-3.5 pb-2.5 pt-1.5 text-sm font-medium",
              "border-b-2 -mb-px transition-colors",
              active
                ? "border-primary text-foreground"
                : "border-transparent text-muted-foreground hover:border-border hover:text-foreground",
            )}
          >
            {Icon && <Icon className="h-4 w-4 shrink-0" />}
            <span>{label}</span>
            {count !== undefined && (
              <span
                className={cn(
                  "rounded-full px-1.5 py-0.5 text-2xs font-semibold tabular-nums",
                  active
                    ? "bg-primary/12 text-primary"
                    : "bg-muted text-muted-foreground group-hover:bg-secondary",
                )}
              >
                {count}
              </span>
            )}
          </button>
        );
      })}
    </div>
  );
}
