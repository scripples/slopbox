import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  // Allow API calls to cb-api on Fly.io
  async rewrites() {
    return [];
  },
};

export default nextConfig;
