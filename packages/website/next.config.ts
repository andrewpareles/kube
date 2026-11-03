import { loadEnvConfig } from "@next/env";
import type { NextConfig } from "next";

loadEnvConfig(process.cwd());

const supabaseUrl = process.env.SUPABASE_URL;
if (!supabaseUrl) {
  throw new Error("Please make sure the variable SUPABASE_URL is set in `.env`.");
}

const supabaseUrlInfo = new URL(supabaseUrl);
if (supabaseUrlInfo.protocol !== "https:") {
  throw new Error("SUPABASE_URL must use https.");
}

const nextConfig: NextConfig = {
  // Browser code can only read NEXT_PUBLIC_ variables, so these are copies of the shared names in .env.
  env: {
    NEXT_PUBLIC_SUPABASE_URL: supabaseUrl,
    NEXT_PUBLIC_SUPABASE_PUBLISHABLE_KEY: process.env.SUPABASE_PUBLISHABLE_KEY,
    NEXT_PUBLIC_SITE_URL: process.env.SITE_URL,
    NEXT_PUBLIC_POSTHOG_KEY: process.env.POSTHOG_KEY,
  },
  outputFileTracingIncludes: {
    // sharp's native addon dlopens libvips, which file tracing cannot follow.
    '/assets/*/texture': [
      './lib/rendering/basis/basis_transcoder.wasm',
      './node_modules/@img/sharp-libvips-*/lib/**'
    ],
    // Usernames, game titles and game descriptions are all checked against the same filter, so any
    // route that writes one needs the file. Listing them one by one would fail at run time the first
    // time somebody added another place a name is accepted.
    '/**': ['./lib/gen/profanity/profanity_bg.wasm'],
  },
  // The home page is the create page. Not permanent, since browsers keep a permanent redirect for good.
  async redirects() {
    return [
      { source: '/', destination: '/create', permanent: false },
      // The docs moved from /api/scripting to /docs. Browsers keep the #anchor across a redirect.
      { source: '/api', destination: '/docs', permanent: false },
      { source: '/api/scripting', destination: '/docs', permanent: false },
    ];
  },
  images: {
    remotePatterns: [
      {
        protocol: "https",
        hostname: "cdn.kube.new",
        pathname: "/game-thumbnails/**",
        search: "",
      },
    ],
  },
};

export default nextConfig;
