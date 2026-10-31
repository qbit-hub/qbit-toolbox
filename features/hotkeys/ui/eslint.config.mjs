import desktopConfig from "../../../apps/desktop/eslint.config.mjs";
import { fileURLToPath } from "node:url";
export default [
  ...desktopConfig,
  {
    rules: {
      "@next/next/no-html-link-for-pages": [
        "error",
        fileURLToPath(new URL("../../../apps/desktop/app", import.meta.url)),
      ],
    },
  },
];
