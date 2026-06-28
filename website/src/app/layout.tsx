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
    default: "Turbo — Fast, Type-Safe, Compiled Language",
    template: "%s — Turbo",
  },
  description:
    "A compiled programming language with JavaScript's developer experience and Rust's performance. Native speed, tiny binaries, zero GC, small core.",
  openGraph: {
    type: "website",
    siteName: "Turbo",
    url: "/",
    title: "Turbo — Fast, Type-Safe, Compiled Language",
    description:
      "JavaScript's soul. Rust's speed. A small, honest core that ships today.",
  },
  twitter: {
    card: "summary_large_image",
    title: "Turbo — Fast, Type-Safe, Compiled Language",
    description:
      "JavaScript's soul. Rust's speed. A small, honest core that ships today.",
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
      className={`${geistSans.variable} ${geistMono.variable} h-full antialiased`}
    >
      <body className="min-h-full flex flex-col bg-background text-foreground">
        <Navbar />
        <main className="flex-1 pt-16">{children}</main>
        <Footer />
      </body>
    </html>
  );
}
