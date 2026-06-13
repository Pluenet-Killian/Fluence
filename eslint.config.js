// Shared flat eslint config (PLAN §0.6: strict eslint, no unjustified `any`
// — covered by strictTypeChecked).
// @ts-check
import tseslint from "typescript-eslint";

export default tseslint.config(
  // generated/: produced by `cargo xtask check-contracts`, drift-checked in
  // CI — lint style does not apply to generated code (tsc still covers it).
  { ignores: ["**/dist/**", "**/node_modules/**", "**/generated/**"] },
  {
    files: [
      "packages/*/src/**/*.ts",
      "packages/*/src/**/*.tsx",
      "apps/*/src/**/*.ts",
      "apps/*/src/**/*.tsx",
    ],
    extends: [...tseslint.configs.strictTypeChecked, ...tseslint.configs.stylisticTypeChecked],
    languageOptions: {
      parserOptions: {
        projectService: true,
        tsconfigRootDir: import.meta.dirname,
      },
    },
  },
);
