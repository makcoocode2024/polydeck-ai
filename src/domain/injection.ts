export type InjectionChannel = "NativeUserScript" | "Cdp" | "None";
export type InjectionStage = "Stopped" | "NativeReady" | "BridgeRunning" | "Unavailable" | "Failed";

export interface InjectStatus {
  stage: InjectionStage;
  channel: InjectionChannel;
  native: {
    available: boolean;
    installed: boolean;
    enabled: boolean;
    healthy: boolean;
    restart_required: boolean;
    script_hash: string | null;
  };
  message: string | null;
}
