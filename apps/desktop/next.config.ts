import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  output: "export",
  reactStrictMode: true,
  transpilePackages: [
    "@qbit/ui",
    "@qbit/ipc",
    "@qbit/i18n",
    "@qbit/hotkeys-ui",
    "md3-next",
  ],
};

export default nextConfig;
