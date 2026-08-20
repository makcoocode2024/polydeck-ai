import { atom } from "jotai";

export type Theme = "light" | "dark" | "system";

const stored = typeof localStorage !== "undefined" ? localStorage.getItem("ai-deck-theme") : null;

export const themeAtom = atom<Theme>((stored as Theme) || "system");
