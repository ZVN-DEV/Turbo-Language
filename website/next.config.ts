import type { NextConfig } from "next";

const nextConfig: NextConfig = {
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
