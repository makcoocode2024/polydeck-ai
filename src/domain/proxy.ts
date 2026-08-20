export interface ProxyToolInfo {
  name: string;
  detected: boolean;
  port: number | null;
  running: boolean;
}

export interface ProxyStatus {
  tools: ProxyToolInfo[];
  activeProxy: string | null;
  httpProxy: string | null;
  httpsProxy: string | null;
}