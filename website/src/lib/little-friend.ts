// Little Friend analytics (littlefriend.io). The site key is public: it only
// names the project the tracker reports to. The tracker sets no cookies, reads
// no form values or page text, and strips query strings.

export const LITTLE_FRIEND_SITE_KEY = "lf_LztUwkPAfmtpgYfcVIjpHj4R";

// Production deployments only, so previews and local dev send nothing.
export const LITTLE_FRIEND_ENABLED = process.env.VERCEL_ENV === "production";

type LittleFriendProps = Record<string, string | number | boolean>;

declare global {
  interface Window {
    lf?: (command: string, ...args: unknown[]) => void;
  }
}

// A named event. Does nothing until the tracker has loaded, and nothing at
// all outside production, where the tag is not rendered.
export function track(name: string, props?: LittleFriendProps) {
  if (typeof window === "undefined") return;
  window.lf?.("track", name, props);
}
