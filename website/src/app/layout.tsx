import type { Metadata } from "next";
import { Geist, Geist_Mono } from "next/font/google";
import Navbar from "@/components/navbar";
import Footer from "@/components/footer";
import { SITE_URL } from "@/lib/site";
import "./globals.css";

const geistSans = Geist({
  variable: "--font-geist-sans",
  subsets: ["latin"],
});

const geistMono = Geist_Mono({
  variable: "--font-geist-mono",
  subsets: ["latin"],
});

export const metadata: Metadata = {
  metadataBase: new URL(SITE_URL),
  title: {
    default: "Turbo — Familiar Code. Native Execution.",
    template: "%s — Turbo",
  },
  description:
    "Familiar code. Native execution. A path to deeper control. Turbo is a compiled language for people who like TypeScript and JavaScript ergonomics and want native binaries.",
  openGraph: {
    type: "website",
    siteName: "Turbo",
    url: "/",
    title: "Turbo — Familiar Code. Native Execution.",
    description:
      "Familiar code. Native execution. A path to deeper control.",
  },
  twitter: {
    card: "summary_large_image",
    title: "Turbo — Familiar Code. Native Execution.",
    description:
      "Familiar code. Native execution. A path to deeper control.",
  },
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html
      lang="en"
      data-scroll-behavior="smooth"
      className={`${geistSans.variable} ${geistMono.variable} h-full antialiased`}
    >
      <body className="min-h-full flex flex-col bg-background text-foreground">
        <a
          href="#main-content"
          className="sr-only focus:not-sr-only focus:fixed focus:top-4 focus:left-4 focus:z-[100] focus:rounded-md focus:border focus:border-border focus:bg-surface focus:px-4 focus:py-2 focus:text-sm focus:font-medium focus:text-foreground focus:font-[family-name:var(--font-geist-sans)]"
        >
          Skip to content
        </a>
        <Navbar />
        <main id="main-content" className="flex-1 pt-16">
          {children}
        </main>
        <Footer />
      </body>
    </html>
  );
}
