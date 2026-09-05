import js from "@eslint/js";
import globals from "globals";
import tseslint from "typescript-eslint";
import reactHooks from "eslint-plugin-react-hooks";
import reactRefresh from "eslint-plugin-react-refresh";

// The repo declared `npm run lint` and CI ran it, but no flat config existed, so
// eslint exited non-zero before linting anything and every later CI step was
// skipped. This is that config.
export default tseslint.config(
  {
    // Build output, dependencies, generated Rust bindings, and local scratch.
    ignores: [
      "dist/**",
      "target/**",
      "node_modules/**",
      "crates/**/bindings/**",
      "src-tauri/gen/**",
      ".scratch/**",
      "releases/**",
      "*.config.js",
    ],
  },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    files: ["**/*.{ts,tsx}"],
    languageOptions: {
      ecmaVersion: 2022,
      globals: globals.browser,
    },
    plugins: {
      "react-hooks": reactHooks,
      "react-refresh": reactRefresh,
    },
    rules: {
      ...reactHooks.configs.recommended.rules,
      "react-refresh/only-export-components": [
        "warn",
        { allowConstantExport: true },
      ],
      // Deliberate discards are common at the Tauri IPC boundary; the underscore
      // prefix is the signal that one is intentional.
      "@typescript-eslint/no-unused-vars": [
        "error",
        { argsIgnorePattern: "^_", varsIgnorePattern: "^_" },
      ],
    },
  },
  {
    // Tests and the Vitest setup file run under Node and touch globals that the
    // browser config does not define.
    files: ["src/test/**/*.{ts,tsx}", "tests/**/*.{ts,tsx}", "*.config.ts"],
    languageOptions: {
      globals: { ...globals.browser, ...globals.node },
    },
  },
);
