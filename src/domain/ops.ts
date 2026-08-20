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