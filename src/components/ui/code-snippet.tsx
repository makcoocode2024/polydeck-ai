import { useState } from "react";
import { Check, Copy } from "lucide-react";
import { cn } from "@/lib/utils";
import { Button } from "./button";

export interface SnippetLine {
  label: string;
  value: string;
  /** Show a length instead of the value. For tokens and keys. */
  secret?: boolean;
}

interface CodeSnippetProps {
  lines: SnippetLine[];
  /**
   * What the copy button puts on the clipboard. Defaults to the lines rendered
   * as `label: value`.
   *
   * Worth passing explicitly: on the clients page the copy button copied only
   * the base URL while the block on screen also showed a token, so a user who
   * pasted what they copied was missing half of what they had been shown.
   */
  copyText?: string;
  className?: string;
}

export function CodeSnippet({ lines, copyText, className }: CodeSnippetProps) {
  const [copied, setCopied] = useState(false);

  const payload =
    copyText ?? lines.map((l) => `${l.label}: ${l.value}`).join("\n");

  const copy = () => {
    navigator.clipboard.writeText(payload).then(
      () => {
        setCopied(true);
        window.setTimeout(() => setCopied(false), 1400);
      },
      () => setCopied(false),
    );
  };

  return (
    <div
      className={cn(
        "relative rounded-md border bg-muted/60 py-2 pl-3 pr-10 font-mono text-xs",
        className,
      )}
    >
      <div className="space-y-1">
        {lines.map(({ label, value, secret }) => (
          <div key={label} className="flex gap-2 leading-relaxed">
            <span className="text-muted-foreground shrink-0">{label}:</span>
            <span className={cn("break-all", secret && "italic text-muted-foreground")}>
              {secret ? `已写入（${value.length} 字符）` : value}
            </span>
          </div>
        ))}
      </div>
      <Button
        type="button"
        variant="ghost"
        size="icon-xs"
        onClick={copy}
        aria-label="复制"
        className="absolute right-1.5 top-1.5"
      >
        {copied ? <Check className="h-3.5 w-3.5 text-success" /> : <Copy className="h-3.5 w-3.5" />}
      </Button>
    </div>
  );
}
