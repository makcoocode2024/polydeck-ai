import { expect, test } from "@playwright/test";

test("启动后可从快速配置进入系统设置", async ({ page }) => {
  await page.goto("/");
  await expect(page).toHaveURL(/\/quick-setup$/);
  await page.getByRole("link", { name: "系统设置" }).click();
  await expect(page).toHaveURL(/\/settings$/);
  await expect(page.getByRole("heading", { name: "系统设置与诊断" })).toBeVisible();
});
