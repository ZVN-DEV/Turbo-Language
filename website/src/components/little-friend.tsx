"use client";

import { useEffect } from "react";
import { track } from "@/lib/little-friend";

// Goals match event names, so links to GitHub send their own named event.
function onClick(event: MouseEvent) {
  if (event.button > 1 || !(event.target instanceof Element)) return;
  const link = event.target.closest("a[href]");
  if (!(link instanceof HTMLAnchorElement)) return;
  if (link.hostname === "github.com") track("github.click");
}

export default function LittleFriendEvents() {
  useEffect(() => {
    document.addEventListener("click", onClick);
    document.addEventListener("auxclick", onClick);
    return () => {
      document.removeEventListener("click", onClick);
      document.removeEventListener("auxclick", onClick);
    };
  }, []);
  return null;
}
