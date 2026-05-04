import js from "@eslint/js";
import tseslint from "typescript-eslint";
import reactHooks from "eslint-plugin-react-hooks";
import reactRefresh from "eslint-plugin-react-refresh";

export default tseslint.config(
  // ── Ignore patterns ──────────────────────────────────────────────────────
  {
    ignores: ["dist/**", "src-tauri/**"],
  },

  // ── Base JS rules ─────────────────────────────────────────────────────────
  js.configs.recommended,

  // ── TypeScript rules ──────────────────────────────────────────────────────
  // Uses type-aware linting for deeper checks (import paths, unused vars, etc.)
  ...tseslint.configs.recommendedTypeChecked,

  // ── React-specific rules ──────────────────────────────────────────────────
  {
    plugins: {
      "react-hooks": reactHooks,
      "react-refresh": reactRefresh,
    },
    rules: {
      ...reactHooks.configs.recommended.rules,
      // Warn when a non-component is exported alongside the component — helps
      // Vite HMR work correctly.
      "react-refresh/only-export-components": ["warn", { allowConstantExport: true }],
    },
  },

  // ── Project-wide TypeScript config ────────────────────────────────────────
  {
    languageOptions: {
      parserOptions: {
        project: ["./tsconfig.json", "./tsconfig.node.json"],
        tsconfigRootDir: import.meta.dirname,
      },
    },
  },

  // ── Custom rule overrides ─────────────────────────────────────────────────
  {
    rules: {
      // Allow unused vars prefixed with _ (standard convention for intentionally unused)
      "@typescript-eslint/no-unused-vars": [
        "error",
        { argsIgnorePattern: "^_", varsIgnorePattern: "^_" },
      ],

      // Allow `any` in a few pragmatic cases — tighten if desired
      "@typescript-eslint/no-explicit-any": "warn",

      // Allow non-null assertions — we use them deliberately in canvas/event handlers
      "@typescript-eslint/no-non-null-assertion": "warn",

      // Allow floating promises from event handlers and fire-and-forget patterns
      // (IPC calls in Zustand actions, RAF callbacks)
      "@typescript-eslint/no-floating-promises": [
        "error",
        { ignoreIIFE: true, ignoreVoid: true },
      ],

      // Require consistent type imports
      "@typescript-eslint/consistent-type-imports": [
        "error",
        { prefer: "type-imports", fixStyle: "inline-type-imports" },
      ],

      // No misused promises (e.g. passing async to onClick without void)
      "@typescript-eslint/no-misused-promises": [
        "error",
        { checksVoidReturn: { attributes: false } },
      ],
    },
  },

  // ── Test-file overrides ───────────────────────────────────────────────────
  // Tests favour terse setup over defensive null-checks: a `getByX()!` that
  // returns null fails the test loudly anyway, which is exactly the signal
  // we want. Same for `async () => {}` arrow bodies that don't await — RTL
  // patterns (`fireEvent`, sync `expect`) often fit cleanly inside an async
  // `it()` for symmetry without actually needing await.
  {
    files: ["**/*.test.ts", "**/*.test.tsx"],
    rules: {
      "@typescript-eslint/no-non-null-assertion": "off",
    },
  },
);
