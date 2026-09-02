import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite-plus";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    tsconfigPaths: true,
  },
  server: {
    port: 3000,
  },

  test: {
    environment: "happy-dom",
    setupFiles: ["./vitest.setup.ts"],
  },

  fmt: {
    printWidth: 120,
    experimentalSortImports: {
      groups: ["builtin", "external", "internal", ["parent", "sibling", "index"]],
      internalPattern: ["@ps/"],
      newlinesBetween: true,
    },
  },

  lint: {
    plugins: ["oxc", "typescript", "import", "unicorn", "react", "vitest"],
    categories: {
      correctness: "error",
      suspicious: "error",
    },
    rules: {
      "import/no-cycle": "error",
      "import/extensions": ["error", { js: "never" }],
      "max-lines": ["error", { max: 1000, skipComments: true, skipBlankLines: true }],
      "max-lines-per-function": ["error", { max: 400, skipComments: true, skipBlankLines: true }],
      "no-nested-ternary": "error",
      "no-unneeded-ternary": "error",
      // Leading underscore marks an intentionally-unused binding (params and
      // values kept for documentation), an established convention here.
      "no-underscore-dangle": "off",
      // Render functions passed to react-markdown's `components` prop are the
      // canonical use case the rule's allowAsProps option exists for.
      "react/no-unstable-nested-components": ["error", { allowAsProps: true }],
      "no-restricted-imports": [
        "error",
        {
          patterns: [
            {
              regex: "^\\.\\./",
              message: "Use absolute imports with @ps/* alias instead of relative paths",
            },
          ],
        },
      ],
      "oxc/no-barrel-file": "error",
      "typescript/no-explicit-any": "error",
      "typescript/explicit-function-return-type": "error",
      "typescript/no-unsafe-type-assertion": "error",
      "typescript/no-base-to-string": "error",
      "typescript/restrict-template-expressions": "error",
      "typescript/no-unnecessary-template-expression": "error",
      "typescript/no-redundant-type-constituents": "error",
      "typescript/no-floating-promises": "off",
      "typescript/unbound-method": "off",
      "unicorn/prefer-node-protocol": "error",
      "import/no-unassigned-import": "off",
      "import/no-named-as-default": "off",
      "unicorn/consistent-function-scoping": "off",
      "unicorn/prefer-add-event-listener": "off",
      "react/react-in-jsx-scope": "off",
      // React Compiler diagnostics are advisory; these established state and
      // ref patterns intentionally opt out of compiler memoization.
      "react/exhaustive-effect-dependencies": "off",
      "react/immutability": "off",
      "react/incompatible-library": "off",
      "react/memo-dependencies": "off",
      "react/purity": "off",
      "react/refs": "off",
      "react/set-state-in-effect": "off",
      "react/static-components": "off",
    },
    options: {
      typeAware: true,
      typeCheck: true,
    },
  },
});
