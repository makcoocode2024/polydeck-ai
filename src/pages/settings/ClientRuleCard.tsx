import type { LucideIcon } from "lucide-react";
import type { ReactNode } from "react";
import { Card, CardHeader, CardTitle, CardContent } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { AlertTriangle, XCircle } from "lucide-react";
import type { ClientRuleStatus } from "@/domain/ops";

interface ClientRuleCardProps {
  icon: LucideIcon;
  title: string;
  /** Derives the card/toggle/error test ids, which the page's tests select on. */
  testIdPrefix: string;
  status: ClientRuleStatus | null;
  error: string | null;
  saving: boolean;
  onToggle: (next: boolean) => void;
  /** Bold line beside the checkbox. */
  label: string;
  description: string;
  /** ReactNode, not string: one caller needs two sentences with its own emphasis. */
  footnote: ReactNode;
}

export function ClientRuleCard({
  icon: Icon,
  title,
  testIdPrefix,
  status,
  error,
  saving,
  onToggle,
  label,
  description,
  footnote,
}: ClientRuleCardProps) {
  return (
    <Card data-testid={`${testIdPrefix}-card`}>
      <CardHeader className="pb-3">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <Icon className="h-4 w-4 text-primary" />
            <CardTitle>{title}</CardTitle>
          </div>
          {error ? (
            <Badge variant="destructive">不可用</Badge>
          ) : status ? (
            <Badge variant={status.enabled ? "success" : "secondary"}>
              {status.enabled ? "已启用" : "已关闭"}
            </Badge>
          ) : (
            <Badge variant="secondary">读取中</Badge>
          )}
        </div>
      </CardHeader>
      <CardContent className="space-y-3 text-xs">
        <label className="flex items-start gap-3 rounded-lg border bg-muted/30 p-3 transition-colors hover:bg-muted/50 cursor-pointer">
          <input
            type="checkbox"
            checked={status?.enabled ?? false}
            disabled={!status || saving}
            onChange={(e) => onToggle(e.target.checked)}
            className="mt-0.5 h-4 w-4 rounded border-input text-primary focus:ring-primary"
            data-testid={`${testIdPrefix}-toggle`}
          />
          <div className="flex-1 space-y-0.5">
            <div className="text-xs font-medium text-foreground">{label}</div>
            <p className="text-2xs text-muted-foreground">{description}</p>
          </div>
        </label>

        {error && (
          <div
            className="space-y-1 rounded-lg border border-destructive/30 bg-destructive-surface p-3"
            data-testid={`${testIdPrefix}-error`}
          >
            <p className="flex items-start gap-1 text-2xs text-destructive">
              <XCircle className="mt-0.5 h-3 w-3 shrink-0" />
              <span>读取失败：{error}</span>
            </p>
            <p className="text-2xs text-muted-foreground">
              若提示命令不存在，说明当前运行的是旧构建。执行 build_release.bat
              重新生成生产构建后重启即可。
            </p>
          </div>
        )}

        {status?.targets.map((t) => (
          <div key={t.path} className="space-y-1">
            <div className="flex items-center justify-between gap-2">
              <span className="font-medium text-foreground">{t.target}</span>
              {t.error ? (
                <Badge variant="destructive">写入失败</Badge>
              ) : (
                <Badge variant={t.rulePresent ? "success" : "secondary"}>
                  {t.rulePresent ? "规则已写入" : "未写入"}
                </Badge>
              )}
            </div>
            <p className="break-all font-mono text-2xs text-muted-foreground">{t.path}</p>
            {t.error && (
              <p className="flex items-start gap-1 text-2xs text-destructive">
                <XCircle className="mt-0.5 h-3 w-3 shrink-0" />
                {t.error}
              </p>
            )}
            {t.shadowedBy && (
              <p className="flex items-start gap-1 text-2xs text-warning">
                <AlertTriangle className="mt-0.5 h-3 w-3 shrink-0" />
                <span>
                  该文件被 <span className="break-all font-mono">{t.shadowedBy}</span>{" "}
                  抢先读取，规则不会生效。删除或清空该文件后才会读到这里。
                </span>
              </p>
            )}
          </div>
        ))}

        <p className="text-2xs text-muted-foreground">{footnote}</p>
      </CardContent>
    </Card>
  );
}
