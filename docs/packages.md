# Installing and Publishing Packages

Turbo ships a small, built-in package manager. Dependencies are declared in
`turbo.toml`, pinned in `turbo.lock`, and installed into a local
`turbo_modules/` directory. There is no central package server and no auth —
packages are ordinary Git repositories, and the registry is a single, curated,
git-versioned JSON file.

The user-facing version of this page is at
[turbolang.dev/docs/packages](https://turbolang.dev/docs/packages); browse
published packages at [turbolang.dev/packages](https://turbolang.dev/packages).

## Declaring dependencies

Add dependencies under `[dependencies]` (or `[dev-dependencies]`) in
`turbo.toml`. Three source forms are supported:

```toml
[dependencies]
# 1. Registry version — a name resolved to a GitHub repo + a matching git tag
turbo-http = "0.1"

# 2. GitHub repo — pin by tag range (version), exact commit (rev), or lockfile
agent-kit = { github = "owner/agent-kit", version = "1.2" }
fixed     = { github = "owner/fixed", rev = "a1b2c3d" }

# 3. Local path — symlinked into turbo_modules (copied on non-Unix)
mylib = { path = "../mylib" }

[dev-dependencies]
turbo-test-utils = "0.1"
```

A version like `"0.1"` selects the latest `0.1.x` tag; a full `"0.1.0"` pins
that exact tag. Tags may be written with or without a leading `v`.

In source, import a package by name — the resolver looks for
`turbo_modules/<name>/src/lib.tb`, then `turbo_modules/<name>/src/<name>.tb`.

Dependency names are validated: they may not contain path separators, `..`
traversal, an absolute path, or a Windows drive prefix, and `turbo_modules/`
itself must stay inside the project root and must not be a symlink.

## `turbolang install`

Reads `turbo.toml`, resolves every dependency, and places it under
`turbo_modules/<name>`:

- **Path** dependencies are symlinked (copied on non-Unix platforms).
- **GitHub** and **registry** dependencies are shallow-cloned and checked out at
  the resolved commit, then recorded in `turbo.lock`.
- If a dependency directory already exists, it is re-pinned to the resolved
  commit rather than re-cloned.

## `turbolang update`

Moves Git-backed dependencies to the newest commit their declaration allows —
the latest tag for a `version`, a fast-forward `git pull` for a bare GitHub
repo — and rewrites `turbo.lock`. Path dependencies are left untouched. If a
dependency isn't installed yet it is skipped with a note to run `install` first.

## The lockfile

`turbo.lock` pins every Git-backed dependency to an exact commit for
reproducible installs. It is generated and overwritten by `install` and
`update` (and removed when there are no Git dependencies):

```toml
[github]
turbo-http = "ZVN-DEV/turbo-http#a1b2c3d4e5f6..."
```

When a dependency lists neither `rev` nor `version`, `install` reuses the commit
recorded here. Commit `turbo.lock` for applications so collaborators get the
exact same dependency tree; libraries typically leave it out.

## Registry resolution

A bare registry dependency (form 1 above) is a *name*, not a repo. Turbo maps
the name to a GitHub repo by checking, in order:

1. An explicit `[registries]` entry in your `turbo.toml` (a per-project
   override).
2. The published registry index at `https://turbolang.dev/registry/index.json`.
3. The built-in default: any `turbo-*` name maps to `ZVN-DEV/<name>`.

```toml
[registries]
# Point a name at any repo you control
cool-lib = "myorg/cool-lib"
```

If the index can't be reached, resolution falls back to the built-in default —
a registry outage never blocks an install. The index is fetched at most once per
`install`/`update`, and only when a bare registry dependency has no explicit
`[registries]` entry.

## `turbolang search`

```bash
turbolang search json        # match name, description, or category
turbolang search             # list every published package
```

`search` fetches the published index and filters it case-insensitively across
name, description, and categories, printing each match with a ready-to-run
`turbolang install` command. It fails gracefully when offline. Point it at a
mirror or a local copy with the `TURBO_REGISTRY_INDEX_URL` environment variable
(only `https://` and `file://` URLs are accepted).

## The registry index

The canonical index lives at `registry/index.json` in the Turbo repository and
is served at `https://turbolang.dev/registry/index.json`. Its schema:

```json
{
  "schema_version": 1,
  "packages": [
    {
      "name": "turbo-json",
      "repo": "owner/turbo-json",
      "description": "A fast JSON parser and serializer",
      "categories": ["serialization"],
      "min_turbo_version": "0.10.0",
      "homepage": "https://example.com"
    }
  ]
}
```

Per entry: `name` (unique, matches the `turbo_modules` directory name), `repo`
(`owner/name` on GitHub), `description`, `categories` (array of lowercase tags),
and the optional `min_turbo_version` (semver the package needs) and `homepage`.

## Publishing a package

There is no account to create and nothing to upload. Publishing is an ordinary
pull request:

1. Ship a Git repo with a `turbo.toml` and a `src/lib.tb` entry point, tagged
   with a semver release (e.g. `v0.1.0`).
2. Open a pull request against `registry/index.json` in the Turbo repository,
   appending one object to the `packages` array.
3. On merge, the entry is served at `https://turbolang.dev/registry/index.json`
   and appears on the packages page and in `turbolang search`.

The index array starts empty on purpose: an entry is only added once the package
resolves as a real `turbo_modules` dependency, so nothing on the page is
"coming soon."
```

