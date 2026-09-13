import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

/**
 * A labelled value, and a grid to lay several out.
 *
 * The extensions page had four of these written as bespoke divs; the profile
 * inspector has its own near-identical pair. Splitting label from value here
 * keeps the label at one size everywhere instead of drifting between 10px and
 * 12px per screen.
 */
export function KeyValue({
  label,
  children,
  mono,
  className,
}: {
  label: string;
  children: ReactNode;
  mono?: boolean;
  className?: string;
}) {
  return (
    <div className={cn("min-w-0 space-y-0.5", className)}>
      <div className="text-2xs uppercase tracking-wide text-muted-foreground">{label}</div>
      <div className={cn("truncate text-sm", mono && "font-mono text-xs")}>{children}</div>
    </div>
  );
}

export function KeyValueGrid({
  children,
  className,
}: {
  children: ReactNode;
  className?: string;
}) {
  return (
    <div className={cn("grid gap-x-4 gap-y-3 sm:grid-cols-2", className)}>{children}</div>
  );
}
