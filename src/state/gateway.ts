import { atom } from "jotai";

export interface GatewayStatus {
  running: boolean;
  port: number | null;
}

export const gatewayStatusAtom = atom<GatewayStatus>({ running: false, port: null });
