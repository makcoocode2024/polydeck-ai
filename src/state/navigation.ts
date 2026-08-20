import { atom } from "jotai";

export type PageId = "quick-setup" | "profiles" | "clients" | "extensions" | "history" | "settings";

export const currentPageAtom = atom<PageId>("quick-setup");
