export type DiagnosticLevel = "error" | "warning" | "ok";

export interface DiagnosticItem {
  category: string;
  level: DiagnosticLevel;
  message: string;
  impact: string;
  suggestion: string;
}

export interface DiagnosticReport {
  items: DiagnosticItem[];
  errors: number;
  warnings: number;
  okCount: number;
  timestamp: string;
}

export interface UpdateInfo {
  available: boolean;
  version: string | null;
}

export interface AutoLaunchStatus {
  enabled: boolean;
  method: string;
}

/** State of one rule in one client's global instructions file. */
export interface ClientRuleTarget {
  target: string;
  path: string;
  /** Whether the block is in the file, which can differ from the app setting. */
  rulePresent: boolean;
  changed: boolean;
  /** Set when another file takes precedence, so the rule will not be read. */
  shadowedBy: string | null;
  /** Set when this client's file could not be handled. */
  error: string | null;
}

/** The saved setting plus what each client's file actually carries. */
export interface ClientRuleStatus {
  enabled: boolean;
  targets: ClientRuleTarget[];
}