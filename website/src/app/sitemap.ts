import type { MetadataRoute } from "next";
import { readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { SITE_URL } from "@/lib/site";

// Enumerate every static route by reading the actual files under src/app so the
// sitemap can't drift from the route tree. Runs at build time (force-static).
export const dynamic = "force-static";

function pageFile(appDir: string, route: string): string {
  if (route === "/") return join(appDir, "page.tsx");
  return join(appDir, ...route.split("/").filter(Boolean), "page.tsx");
}

export default function sitemap(): MetadataRoute.Sitemap {
  const appDir = join(process.cwd(), "src", "app");
  const docsDir = join(appDir, "docs");

  const docsRoutes = readdirSync(docsDir, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .filter((entry) => {
      try {
        return statSync(join(docsDir, entry.name, "page.tsx")).isFile();
      } catch {
        return false;
      }
    })
    .map((entry) => `/docs/${entry.name}`)
    .sort();

  const routes = ["/", "/play", "/docs", ...docsRoutes];

  return routes.map((route) => ({
    url: `${SITE_URL}${route === "/" ? "" : route}`,
    lastModified: statSync(pageFile(appDir, route)).mtime,
    changeFrequency: "weekly",
    priority: route === "/" ? 1 : 0.7,
  }));
}
