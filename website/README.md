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

For the hosted playground, use the scripted smoke once the sandbox runner is
configured:

```bash
TURBO_PLAYGROUND_SITE_URL=https://turbolang.dev npm run smoke:playground
```

The smoke checks the public `/play` page, a safe execution through
`/api/playground/run`, and rejection of the forbidden `exec` process API.

### Hosted Playground Execution

The `/play` page is static, but live execution is intentionally delegated to a
separate sandbox runner. The Next.js API only accepts JSON, validates the source
envelope, and forwards to that runner; do not add shell execution to the app.

Build the runner from the repository root:

```bash
docker build -t turbo-playground-runner -f website/playground-runner/Dockerfile .
```

Run it with the hardening flags documented in
[`playground-runner/README.md`](playground-runner/README.md), then configure the
website runtime:

```bash
TURBO_PLAYGROUND_RUNNER_URL=https://runner.example.com/run
TURBO_PLAYGROUND_RUNNER_TOKEN=...
```

The runner token is trimmed before use; whitespace-only values are treated as
missing.

If those variables are absent, `/api/playground/run` returns an explicit
unavailable response and the page shows the local CLI command instead.

## Content Ownership

When compiler, CLI, release, or packaging behavior changes, update the matching website pages in `website/src/app/**` in the same cycle or record the doc gap in `.omx/product-cycles/open-tasks.md`.
