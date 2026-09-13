import { cn } from "@/lib/utils";
import type { LucideIcon } from "lucide-react";
import { Card, CardContent } from "./card";

/**
 * A metric with a tinted icon square, as the history page's three headline
 * numbers use.
 *
 * `tone` picks a semantic token rather than a raw palette entry — those three
 * cards were `bg-primary/10`, `bg-sky-500/10` and `bg-amber-500/10`, so only one
 * of them tracked the theme and the other two had no dark-mode counterpart.
 */
const toneClasses = {
  primary: "bg-primary/10 text-primary",
  info: "bg-info-surface text-info",
  success: "bg-success-surface text-success",
  warning: "bg-warning-surface text-warning",
  neutral: "bg-muted text-muted-foreground",
} as const;

interface StatTileProps {
  label: string;
  value: string | number;
  icon: LucideIcon;
  tone?: keyof typeof toneClasses;
  /** Optional context under the number, e.g. a comparison or unit note. */
  hint?: string;
  className?: string;
}

export function StatTile({
  label,
  value,
  icon: Icon,
  tone = "neutral",
  hint,
  className,
}: StatTileProps) {
  return (
    <Card className={className}>
      <CardContent className="flex items-center gap-3.5 p-4">
        <div className={cn("rounded-lg p-2.5 shrink-0", toneClasses[tone])}>
          <Icon className="h-5 w-5" />
        </div>
        <div className="min-w-0">
          <div className="text-xs text-muted-foreground truncate">{label}</div>
          <div className="text-xl font-semibold leading-tight tabular-nums">{value}</div>
          {hint && <div className="text-2xs text-muted-foreground truncate">{hint}</div>}
        </div>
      </CardContent>
    </Card>
  );
}
