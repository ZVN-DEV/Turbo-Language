import { ImageResponse } from "next/og";

// Default social card for every route. Next.js wires this file into the
// og:image (and twitter:image) tags site-wide via the file-based metadata
// convention, so a shared link renders a branded card instead of bare text.
export const alt = "Turbo — JavaScript's Soul. Rust's Speed.";
export const size = { width: 1200, height: 630 };
export const contentType = "image/png";

export default function OpengraphImage() {
  return new ImageResponse(
    (
      <div
        style={{
          width: "100%",
          height: "100%",
          display: "flex",
          flexDirection: "column",
          justifyContent: "space-between",
          backgroundColor: "#0a0a0a",
          padding: "80px",
          fontFamily: "sans-serif",
        }}
      >
        {/* Wordmark */}
        <div style={{ display: "flex", alignItems: "center", gap: "16px" }}>
          <div
            style={{
              width: "28px",
              height: "28px",
              borderRadius: "9999px",
              backgroundColor: "#00ff88",
              display: "flex",
            }}
          />
          <div
            style={{
              display: "flex",
              fontSize: "36px",
              fontWeight: 700,
              color: "#ffffff",
            }}
          >
            Turbo
          </div>
        </div>

        {/* Tagline */}
        <div style={{ display: "flex", flexDirection: "column" }}>
          <div
            style={{
              display: "flex",
              fontSize: "92px",
              fontWeight: 800,
              color: "#ffffff",
              lineHeight: 1.05,
            }}
          >
            JavaScript&apos;s Soul.
          </div>
          <div
            style={{
              display: "flex",
              fontSize: "92px",
              fontWeight: 800,
              color: "#00ff88",
              lineHeight: 1.05,
            }}
          >
            Rust&apos;s Speed.
          </div>
          <div
            style={{
              display: "flex",
              marginTop: "28px",
              fontSize: "32px",
              color: "#9ca3af",
            }}
          >
            A compiled, type-safe language. No VM, no GC, tiny binaries.
          </div>
        </div>

        {/* Accent bar + url */}
        <div style={{ display: "flex", flexDirection: "column", gap: "20px" }}>
          <div
            style={{
              display: "flex",
              height: "6px",
              width: "220px",
              borderRadius: "9999px",
              backgroundImage: "linear-gradient(90deg, #00ff88, #00d4ff)",
            }}
          />
          <div style={{ display: "flex", fontSize: "30px", color: "#6b7280" }}>
            turbolang.dev
          </div>
        </div>
      </div>
    ),
    { ...size },
  );
}
