# Turbo Website

This is the Next.js app for the Turbo language website. It contains the landing page, installation docs, examples, CLI reference, and product pages that should stay aligned with the compiler/toolchain in this repository.

## Local Development

Use any supported local Node/npm install. Inside Codex App, start with the repository automation PATH, but use Homebrew Node/npm for `npm run build` if native `.node` bindings fail to load with a macOS code-signature Team ID error.

```bash
npm ci
npm run dev
```

Open <http://localhost:3000>.

## Verification

Run the same gate CI and nightly use for the website:

```bash
npm run lint
npm run build
npm audit --audit-level=high
```

As of the 2026-06-27 product cycle, the high-severity audit gate passes. `npm audit` still reports known low/moderate transitive findings in the Next.js toolchain; do not treat those as a high-severity release blocker without a fresh audit result.

## Deployment

The local checkout is already linked to Vercel metadata in `.vercel/project.json`:

- project: `website`
- project id: `prj_jvXQLV6BMMoUm2vkT7UOIUil2bFc`
- org id: `team_zCLdY1qPIO8Foz3XjEbNkGBg`

Do not commit Vercel tokens or print deployment secrets. A deploy-capable shell should use the Vercel CLI with account credentials outside the repository:

```bash
npm run build
npx vercel deploy --prebuilt
```

After deployment, smoke the public or preview URL by checking the landing page, installation page, CLI docs, examples page, and any route changed in the commit.

## Content Ownership

When compiler, CLI, release, or packaging behavior changes, update the matching website pages in `website/src/app/**` in the same cycle or record the doc gap in `.omx/product-cycles/open-tasks.md`.
