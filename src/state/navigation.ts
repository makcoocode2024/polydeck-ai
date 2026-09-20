import { atom } from "jotai";

export type PageId = "quick-setup" | "profiles" | "clients" | "extensions" | "history" | "smart-route" | "settings";

export const currentPageAtom = atom<PageId>("quick-setup");
