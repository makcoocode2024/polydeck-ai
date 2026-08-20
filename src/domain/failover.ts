export type ProviderHealth = "healthy" | "degraded" | "failed" | "circuit_open" | "unknown";
export type CircuitState = "closed" | "open" | "half_open";

export interface HealthStatus {
  provider_id: string;
  state: ProviderHealth;
  circuit: CircuitState;
  consecutive_failures: number;
  consecutive_successes: number;
  latency_ms: number | null;
  last_checked_at: string | null;
  last_error: string | null;
}

export interface FailoverStatus {
  profile_id: string;
  running: boolean;
  primary_provider_id: string;
  current_provider_id: string;
  on_backup: boolean;
  all_providers_failed: boolean;
  providers: HealthStatus[];
}
