import { lazy, Suspense, useEffect } from "react";
import { BrowserRouter, Routes, Route, Navigate } from "react-router-dom";
import { useAtomValue } from "jotai";
import { themeAtom } from "./state/theme";
import { Sidebar } from "./components/sidebar";
import { StatusBar } from "./components/status-bar";
import { PageSkeleton } from "./components/ui/page-skeleton";
import { pageLoaders, preloadAllPages } from "./lib/page-preload";

const QuickSetupPage = lazy(pageLoaders.quickSetup);
const ProfilesPage = lazy(pageLoaders.profiles);
const ClientsPage = lazy(pageLoaders.clients);
const ExtensionsPage = lazy(pageLoaders.extensions);
const HistoryPage = lazy(pageLoaders.history);
const SettingsPage = lazy(pageLoaders.settings);

export default function App() {
  const theme = useAtomValue(themeAtom);

  useEffect(() => {
    // Warm route chunks shortly after the first paint so menu switches avoid chunk waits.
    const preloadTimer = window.setTimeout(() => preloadAllPages(), 350);
    return () => window.clearTimeout(preloadTimer);
  }, []);

  useEffect(() => {
    const root = document.documentElement;
    localStorage.setItem("ai-deck-theme", theme);

    const applyTheme = () => {
      if (theme === "dark") {
        root.classList.add("dark");
      } else if (theme === "light") {
        root.classList.remove("dark");
      } else {
        const prefersDark = window.matchMedia("(prefers-color-scheme: dark)").matches;
        if (prefersDark) {
          root.classList.add("dark");
        } else {
          root.classList.remove("dark");
        }
      }
    };

    applyTheme();

    if (theme === "system") {
      const mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");
      const listener = () => applyTheme();
      mediaQuery.addEventListener("change", listener);
      return () => mediaQuery.removeEventListener("change", listener);
    }
  }, [theme]);

  return (
    <BrowserRouter>
      <div className="flex h-screen bg-background text-foreground overflow-hidden font-sans">
        <Sidebar />
        <main className="flex-1 flex flex-col min-w-0 overflow-hidden">
          <div className="flex-1 overflow-y-auto p-6 md:p-8">
            <Suspense fallback={<PageSkeleton />}>
              <Routes>
                <Route path="/" element={<Navigate to="/quick-setup" replace />} />
                <Route path="/quick-setup" element={<QuickSetupPage />} />
                <Route path="/profiles" element={<ProfilesPage />} />
                <Route path="/clients" element={<ClientsPage />} />
                <Route path="/extensions" element={<ExtensionsPage />} />
                <Route path="/history" element={<HistoryPage />} />
                <Route path="/settings" element={<SettingsPage />} />
              </Routes>
            </Suspense>
          </div>
          <StatusBar />
        </main>
      </div>
    </BrowserRouter>
  );
}