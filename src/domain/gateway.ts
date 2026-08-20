export interface GatewayConfig {
  listen_addr: string | null;
  upstream_base_url: string;
  protocol: string;
  model_rewrites: ModelRewriteRule[];
  timeout_secs: number;
  max_retries: number;
  responses_mode: "auto" | "native" | "bridge";
}

export interface ModelRewriteRule {
  from: string;
  to: string;
  enabled: boolean;
  description: string | null;
}
