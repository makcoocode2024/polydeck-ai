import { useAtom } from "jotai";
import { themeAtom, type Theme } from "@/state/theme";
import { Moon, Sun, Laptop } from "lucide-react";
import { Button } from "@/components/ui/button";

export function ThemeToggle() {
  const [theme, setTheme] = useAtom(themeAtom);

  const nextTheme: Record<Theme, Theme> = {
    light: "dark",
    dark: "system",
    system: "light",
  };

  const icons = {
    light: <Sun className="h-4 w-4 text-amber-500" />,
    dark: <Moon className="h-4 w-4 text-sky-400" />,
    system: <Laptop className="h-4 w-4 text-muted-foreground" />,
  };

  const labels = {
    light: "明亮模式",
    dark: "暗黑模式",
    system: "跟随系统",
  };

  return (
    <Button
      variant="ghost"
      size="sm"
      onClick={() => setTheme(nextTheme[theme])}
      className="h-8 w-8 p-0 rounded-md hover:bg-accent"
      title={`当前主题: ${labels[theme]} (点击切换)`}
      aria-label="切换主题"
    >
      {icons[theme]}
    </Button>
  );
}