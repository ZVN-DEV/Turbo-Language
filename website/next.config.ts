import type { NextConfig } from "next";

const isDev = process.env.NODE_ENV !== "production";
const playgroundContentSecurityPolicy = [
  "default-src 'self'",
  "script-src 'self' 'unsafe-inline'" + (isDev ? " 'unsafe-eval'" : ""),
  "style-src 'self' 'unsafe-inline'",
  "img-src 'self' data: blob:",
  "font-src 'self'",
  "connect-src 'self'" + (isDev ? " ws: http:" : ""),
  "object-src 'none'",
  "base-uri 'self'",
  "form-action 'none'",
  "frame-ancestors 'none'",
].join("; ");

const playgroundSecurityHeaders = [
  {
    key: "Content-Security-Policy",
    value: playgroundContentSecurityPolicy,
  },
  {
    key: "Referrer-Policy",
    value: "strict-origin-when-cross-origin",
  },
  {
    key: "Permissions-Policy",
    value: "camera=(), microphone=(), geolocation=()",
  },
  {
    key: "X-Content-Type-Options",
    value: "nosniff",
  },
];

const nextConfig: NextConfig = {
  async headers() {
    return [
      {
        source: "/play",
        headers: playgroundSecurityHeaders,
      },
    ];
  },

  async redirects() {
    return [
      {
        // turbolang.dev/errors/E0NNN -> the canonical error doc (single source
        // of truth lives in the compiler repo). Makes the short, branded error
        // URL resolve instead of 404ing. Only E-prefixed numeric codes match.
        source: "/errors/:code(E[0-9]+)",
        destination:
          "https://github.com/ZVN-DEV/Turbo-Language/blob/master/docs/errors/:code.md",
        permanent: false,
      },
    ];
  },
};

export default nextConfig;
