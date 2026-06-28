// Single source of truth for the site's production origin. Used by the root
// metadata (metadataBase), the sitemap, and robots so the domain can't drift
// across files. Confirmed against repo references (turbolang.dev/errors,
// turbolang.dev/keys, turbolang.dev/play).
export const SITE_URL = "https://turbolang.dev";
