// Docs drift detector — guards against the website's hand-written TSX doc pages
// silently diverging from the canonical Markdown docs (docs/stdlib.md, docs/errors.md)
// and the compiler's own source of truth (turbo-ast/src/errors.rs).
//
// WHY THIS EXISTS
// ---------------
// The canonical reference is Markdown (docs/*.md); the site under
// website/src/app/docs/**/page.tsx is hand-authored TSX. Two independent
// sources drift. This test does NOT render or rebuild the pages — it does a
// cheap textual parse of both sides and cross-checks two things that can be
// verified mechanically with low false-positive risk:
//
//   1. Builtins   — function names in docs/stdlib.md vs builtin-shaped mentions
//                   on the site.
//   2. Error codes — the E0NNN table in docs/errors.md (+ the compiler's
//                   errors.rs) vs the codes listed on the site's error page.
//
// HARD vs SOFT (deliberate, to keep the gate stable rather than flaky)
// --------------------------------------------------------------------
//   HARD FAIL  = the site documents something WRONG: a builtin or error code
//                that does not exist in any canonical source. That is always a
//                real bug on the site and should block.
//   SOFT WARN  = the site is INCOMPLETE: a canonical builtin / error code that
//                the site doesn't mention. The site intentionally covers only a
//                subset (there is no full stdlib page today), so this is
//                reported as a warning list, never a failure.
//
// HOW TO UPDATE WHEN DOC STRUCTURE CHANGES
// ----------------------------------------
//   * Builtin inventory is parsed from (a) the "## Quick Reference" table's
//     `backtick` tokens and (b) the `### heading` names in docs/stdlib.md. If
//     that file's structure changes, adjust parseStdlibBuiltins() below. The
//     non-empty sanity assertions will fail loudly if a structure change makes
//     the parser silently extract nothing.
//   * The BUILTIN_FAMILY_PREFIXES list decides which unknown site tokens are
//     "builtin-shaped" enough to hard-fail on. These are turbo-specific
//     namespaces. Add a prefix here when the stdlib grows a new namespaced
//     family; keep it precise (prefer a trailing `_`) to avoid catching a
//     page's own helper functions.

import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";

// process.cwd() is the website/ dir when run via `npm test`; the canonical
// Markdown docs and the compiler crate live one level up at the repo root.
const websiteRoot = process.cwd();
const repoRoot = join(websiteRoot, "..");
const DOCS_PAGES_DIR = join(websiteRoot, "src/app/docs");

function readRepo(relPath) {
  return readFileSync(join(repoRoot, relPath), "utf8");
}

// Turbo-specific builtin namespaces. Any site token that starts with one of
// these but is NOT in the canonical inventory is treated as documenting a
// nonexistent/renamed builtin (hard fail). Trailing underscores keep these
// from matching a page's own lowercase helper functions.
const BUILTIN_FAMILY_PREFIXES = [
  "str_",
  "hashmap_",
  "http_",
  "json_",
  "to_json",
  "request_",
  "respond_",
  "try_",
  "time_",
  "env_",
  "path_",
  "file_",
  "list_",
  "char_",
  "pad_",
  "index_",
  "starts_",
  "ends_",
  "mutex_",
];

// ---------------------------------------------------------------------------
// Extractors
// ---------------------------------------------------------------------------

// Canonical builtin inventory from docs/stdlib.md.
// Assumptions:
//   - The "## Quick Reference" section lists every builtin as `backtick` tokens
//     inside a Markdown table. This is the flat, authoritative list.
//   - Each builtin also has a "### name" section heading. We union both so an
//     internal inconsistency in stdlib.md (a heading missing from Quick
//     Reference, or vice-versa) still yields a complete canonical set.
//   - "### len (string)" / "### len (array)" collapse to `len` (first word).
function parseStdlibBuiltins() {
  const md = readRepo("docs/stdlib.md");
  const builtins = new Set();

  const qrIndex = md.indexOf("## Quick Reference");
  assert.ok(qrIndex !== -1, "docs/stdlib.md is missing its '## Quick Reference' section — update parseStdlibBuiltins()");
  const quickRef = md.slice(qrIndex);
  for (const m of quickRef.matchAll(/`([a-z][a-z0-9_]*)`/g)) {
    builtins.add(m[1]);
  }

  for (const m of md.matchAll(/^### ([a-z][a-z0-9_]*)/gm)) {
    builtins.add(m[1]);
  }

  return builtins;
}

// Every site doc page's TSX source, keyed by page slug.
function readSiteDocPages() {
  const pages = new Map();
  for (const entry of readdirSync(DOCS_PAGES_DIR, { withFileTypes: true })) {
    if (!entry.isDirectory()) continue;
    const pagePath = join(DOCS_PAGES_DIR, entry.name, "page.tsx");
    try {
      pages.set(entry.name, readFileSync(pagePath, "utf8"));
    } catch {
      // Not every docs subdirectory has a page.tsx (e.g. layout-only). Skip.
    }
  }
  return pages;
}

// Call-like lowercase tokens (`name(`) mentioned on each site page.
// Lowercase-first excludes React components (CodeBlock, etc.). We only ever
// use these to detect builtin-shaped names, so prose false positives are
// harmless unless they collide with a builtin family prefix.
function extractSiteCallTokens(pages) {
  const tokenToPages = new Map();
  for (const [slug, src] of pages) {
    for (const m of src.matchAll(/\b([a-z][a-z0-9_]*)\s*\(/g)) {
      const token = m[1];
      if (!tokenToPages.has(token)) tokenToPages.set(token, new Set());
      tokenToPages.get(token).add(slug);
    }
  }
  return tokenToPages;
}

function looksLikeBuiltinFamily(token) {
  return BUILTIN_FAMILY_PREFIXES.some((p) => token.startsWith(p));
}

// Canonical error codes. Union of:
//   - docs/errors.md table rows "| E0NNN |" (the doc the site should mirror)
//   - turbo-ast/src/errors.rs code mentions (the compiler source of truth)
// Union means a HARD fail only fires when the site lists a code that exists in
// NEITHER — i.e. a truly phantom code — rather than punishing the site when
// errors.md alone is momentarily stale.
// Range annotations ("E0001-E0099", "E0100--E0199") are stripped first so their
// boundary numbers aren't mistaken for real codes.
function stripRanges(text) {
  return text.replace(/E0\d{3}\s*-{1,2}\s*E0\d{3}/g, "");
}

function parseCanonicalErrorCodes() {
  const errorsMd = readRepo("docs/errors.md");
  const fromMdTable = new Set();
  for (const m of errorsMd.matchAll(/^\|\s*(E0\d{3})\s*\|/gm)) {
    fromMdTable.add(m[1]);
  }

  const errorsRs = stripRanges(readRepo("turbo/crates/turbo-ast/src/errors.rs"));
  const fromRs = new Set();
  for (const m of errorsRs.matchAll(/\bE0\d{3}\b/g)) {
    fromRs.add(m[0]);
  }

  return { fromMdTable, union: new Set([...fromMdTable, ...fromRs]) };
}

// Error codes the site's error-codes page presents as real, individual codes.
// Range headers ("E0400--E0499") are stripped so section boundaries aren't
// counted as documented codes.
function parseSiteErrorCodes() {
  const src = stripRanges(readFileSync(join(DOCS_PAGES_DIR, "error-codes/page.tsx"), "utf8"));
  const codes = new Set();
  for (const m of src.matchAll(/\bE0\d{3}\b/g)) {
    codes.add(m[0]);
  }
  return codes;
}

// ---------------------------------------------------------------------------
// Builtins
// ---------------------------------------------------------------------------

test("stdlib builtin inventory parses (guards against a silent parser break)", () => {
  const canonical = parseStdlibBuiltins();
  // If the doc structure changes such that we extract nothing, the parity
  // checks below would vacuously pass. Fail loudly instead.
  assert.ok(canonical.size > 50, `expected a large canonical builtin set, got ${canonical.size} — did docs/stdlib.md structure change?`);
  assert.ok(canonical.has("print"), "canonical builtins missing 'print' — parser likely broke");
  assert.ok(canonical.has("hashmap_get"), "canonical builtins missing 'hashmap_get' — parser likely broke");
});

test("HARD: site does not document a builtin that is absent from docs/stdlib.md", () => {
  const canonical = parseStdlibBuiltins();
  const siteTokens = extractSiteCallTokens(readSiteDocPages());

  const phantom = [];
  for (const [token, slugs] of siteTokens) {
    if (canonical.has(token)) continue;
    // Only hard-fail on tokens that are unmistakably builtin-shaped
    // (turbo-namespaced). A bare word like `map`/`len` in prose is ignored;
    // a `hashmap_gets` typo or a removed/renamed `str_*` builtin is caught.
    if (looksLikeBuiltinFamily(token)) {
      phantom.push(`${token} (mentioned in: ${[...slugs].sort().join(", ")})`);
    }
  }

  assert.deepEqual(
    phantom,
    [],
    `Website documents builtin(s) not present in docs/stdlib.md:\n  - ${phantom.join("\n  - ")}\n` +
      `Either the builtin was renamed/removed (fix the site) or docs/stdlib.md is missing it (add it there).`,
  );
});

test("SOFT: report canonical builtins not mentioned anywhere on the site", () => {
  const canonical = parseStdlibBuiltins();
  const siteTokens = extractSiteCallTokens(readSiteDocPages());

  // Bare structural heading tokens that aren't real callable builtins.
  const notRealBuiltins = new Set(["respond"]); // "respond" heading covers respond_text/html/json
  const missing = [...canonical]
    .filter((b) => !notRealBuiltins.has(b) && !siteTokens.has(b))
    .sort();

  if (missing.length > 0) {
    console.warn(
      `[docs-parity][warn] ${missing.length} canonical builtin(s) are not shown on the website ` +
        `(informational — the site covers a curated subset, no full stdlib page yet):\n  ${missing.join(", ")}`,
    );
  }
  // Report-only: never fails. Present so the warning list is regenerated each run.
  assert.ok(true);
});

// ---------------------------------------------------------------------------
// Error codes
// ---------------------------------------------------------------------------

test("HARD: site does not list an error code absent from docs/errors.md AND errors.rs", () => {
  const { union } = parseCanonicalErrorCodes();
  const siteCodes = parseSiteErrorCodes();

  const phantom = [...siteCodes].filter((c) => !union.has(c)).sort();

  assert.deepEqual(
    phantom,
    [],
    `Website's error-codes page lists code(s) that exist in neither docs/errors.md nor ` +
      `turbo-ast/src/errors.rs:\n  ${phantom.join(", ")}\n` +
      `These are phantom codes — remove them from the site or add them to the canonical sources.`,
  );
});

test("SOFT: report canonical error codes (docs/errors.md) missing from the site", () => {
  const { fromMdTable } = parseCanonicalErrorCodes();
  const siteCodes = parseSiteErrorCodes();

  const missing = [...fromMdTable].filter((c) => !siteCodes.has(c)).sort();
  if (missing.length > 0) {
    console.warn(
      `[docs-parity][warn] ${missing.length} error code(s) documented in docs/errors.md ` +
        `are not listed on the website error-codes page:\n  ${missing.join(", ")}`,
    );
  }
  assert.ok(true);
});
