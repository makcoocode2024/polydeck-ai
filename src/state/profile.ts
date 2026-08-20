import { atom } from "jotai";
import type { Profile, ProfileTemplate } from "@/domain/profile";
import type { DetectedClient } from "@/domain/client";

export const profilesAtom = atom<Profile[]>([]);
export const templatesAtom = atom<ProfileTemplate[]>([]);
export const clientsAtom = atom<DetectedClient[]>([]);
export const activeProfileIdAtom = atom<string | null>(null);