import type { ComponentType } from "react";

export type PageModule = { default: ComponentType };

export const pageLoaders = {
  quickSetup: () => import("@/pages/QuickSetupPage"),
  profiles: () => import("@/pages/ProfilesPage"),
  clients: () => import("@/pages/ClientsPage"),
  extensions: () => import("@/pages/ExtensionsPage"),
  history: () => import("@/pages/HistoryPage"),
  settings: () => import("@/pages/SettingsPage"),
} satisfies Record<string, () => Promise<PageModule>>;

const preloaded = new Set<string>();

export function preloadPage(page: keyof typeof pageLoaders) {
  if (preloaded.has(page)) return;
  preloaded.add(page);
  void pageLoaders[page]().catch(() => {
    // Allow a later navigation to retry after a transient chunk failure.
    preloaded.delete(page);
  });
}

export function preloadAllPages() {
  (Object.keys(pageLoaders) as Array<keyof typeof pageLoaders>).forEach(preloadPage);
}
