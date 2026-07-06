import type { Metadata } from "next";
import PlaygroundClient from "@/components/playground-client";

export const metadata: Metadata = {
  title: "Playground",
  description:
    "Try Turbo syntax in a hosted browser editor, load runnable examples, and copy a local Turbo command.",
  openGraph: {
    title: "Turbo Playground",
    description:
      "Try Turbo syntax in a hosted browser editor, load runnable examples, and copy a local Turbo command.",
    url: "/play",
  },
  twitter: {
    title: "Turbo Playground",
    description:
      "Try Turbo syntax in a hosted browser editor, load runnable examples, and copy a local Turbo command.",
  },
};

export default function PlayPage() {
  return <PlaygroundClient />;
}
