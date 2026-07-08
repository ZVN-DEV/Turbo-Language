//! Package registry index: fetching, parsing, searching, and name->repo
//! resolution against the curated static index published at
//! `https://turbolang.dev/registry/index.json`.
//!
//! The index is a static, git-versioned JSON file (canonical source lives at
//! `registry/index.json` in the Turbo repo). There is no dynamic registry
//! service and no auth — publishing is a pull request that appends an entry.
//! `turbolang search` fetches and filters it; `turbolang install` consults it
//! to map a bare package name to its GitHub repo before falling back to the
//! `turbo-*` -> `ZVN-DEV/{name}` default.

use std::io::Write;

/// Production URL for the published index. Overridable via the
/// `TURBO_REGISTRY_INDEX_URL` env var (used by tests and by anyone pointing at
/// a mirror or a local `file://` copy).
const DEFAULT_INDEX_URL: &str = "https://turbolang.dev/registry/index.json";

/// A single package entry in the registry index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RegistryPackage {
    pub name: String,
    pub repo: String,
    pub description: String,
    pub categories: Vec<String>,
    pub min_turbo_version: Option<String>,
    pub homepage: Option<String>,
    /// Optional subdirectory within `repo` that holds the package (a
    /// `turbo.toml` + `src/lib.tb`). Set for monorepo packages so `install`
    /// clones `repo` at the requested tag and installs from this subdirectory
    /// instead of the repository root. Absent for single-package repos.
    pub subdir: Option<String>,
}

/// The parsed registry index.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct RegistryIndex {
    pub schema_version: u64,
    pub packages: Vec<RegistryPackage>,
}

/// Resolve the index URL to fetch, honoring the `TURBO_REGISTRY_INDEX_URL`
/// override.
pub(crate) fn registry_index_url() -> String {
    std::env::var("TURBO_REGISTRY_INDEX_URL").unwrap_or_else(|_| DEFAULT_INDEX_URL.to_string())
}

/// Fetch the raw index body from `url`.
///
/// Only `https://` and `file://` URLs are accepted — plain `http://` and other
/// schemes are rejected so an env override can't downgrade the transport or
/// point the tool at an unexpected scheme. `file://` is read directly (hermetic,
/// no subprocess); `https://` is fetched by shelling out to `curl`, mirroring
/// the way `deps.rs` shells out to `git` rather than pulling in an HTTP client.
pub(crate) fn fetch_registry_index(url: &str) -> Result<String, String> {
    if let Some(path) = url.strip_prefix("file://") {
        return std::fs::read_to_string(path)
            .map_err(|e| format!("could not read local registry index `{path}`: {e}"));
    }

    if !url.starts_with("https://") {
        return Err(format!(
            "refusing to fetch registry index from `{url}`: only https:// and file:// URLs are allowed"
        ));
    }

    let output = std::process::Command::new("curl")
        .args(["-fsSL", "--max-time", "15", url])
        .output()
        .map_err(|e| format!("could not run curl to fetch the registry index: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = stderr.trim();
        if detail.is_empty() {
            return Err(format!("failed to fetch registry index from {url}"));
        }
        return Err(format!(
            "failed to fetch registry index from {url}: {detail}"
        ));
    }

    String::from_utf8(output.stdout).map_err(|_| "registry index was not valid UTF-8".to_string())
}

/// Parse the JSON index body into a [`RegistryIndex`]. Unknown/missing optional
/// fields are tolerated; malformed structure is a hard error.
pub(crate) fn parse_registry_index(body: &str) -> Result<RegistryIndex, String> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("registry index is not valid JSON: {e}"))?;

    let schema_version = value
        .get("schema_version")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| "registry index is missing an integer `schema_version`".to_string())?;

    let packages_value = value
        .get("packages")
        .ok_or_else(|| "registry index is missing a `packages` array".to_string())?;
    let packages_arr = packages_value
        .as_array()
        .ok_or_else(|| "registry index `packages` must be an array".to_string())?;

    let mut packages = Vec::with_capacity(packages_arr.len());
    for (i, entry) in packages_arr.iter().enumerate() {
        let field = |key: &str| -> Result<String, String> {
            entry
                .get(key)
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .ok_or_else(|| format!("package #{i} in registry index is missing string `{key}`"))
        };
        let categories = entry
            .get("categories")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let opt = |key: &str| -> Option<String> {
            entry
                .get(key)
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        };
        packages.push(RegistryPackage {
            name: field("name")?,
            repo: field("repo")?,
            description: field("description")?,
            categories,
            min_turbo_version: opt("min_turbo_version"),
            homepage: opt("homepage"),
            subdir: opt("subdir"),
        });
    }

    Ok(RegistryIndex {
        schema_version,
        packages,
    })
}

/// Case-insensitive substring match of `query` against a package's name,
/// description, and categories. An empty query matches everything.
pub(crate) fn package_matches(pkg: &RegistryPackage, query: &str) -> bool {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return true;
    }
    if pkg.name.to_lowercase().contains(&needle) {
        return true;
    }
    if pkg.description.to_lowercase().contains(&needle) {
        return true;
    }
    pkg.categories
        .iter()
        .any(|c| c.to_lowercase().contains(&needle))
}

/// Filter the index's packages by `query`, returning matches sorted by name.
pub(crate) fn filter_packages<'a>(
    index: &'a RegistryIndex,
    query: &str,
) -> Vec<&'a RegistryPackage> {
    let mut matches: Vec<&RegistryPackage> = index
        .packages
        .iter()
        .filter(|pkg| package_matches(pkg, query))
        .collect();
    matches.sort_by(|a, b| a.name.cmp(&b.name));
    matches
}

/// Look up a bare package name in the index and return its `owner/name` repo.
pub(crate) fn resolve_repo_from_index(name: &str, index: &RegistryIndex) -> Option<String> {
    index
        .packages
        .iter()
        .find(|pkg| pkg.name == name)
        .map(|pkg| pkg.repo.clone())
}

/// Look up a bare package name in the index and return its `subdir`, if the
/// entry is a monorepo package (an entry with no `subdir` returns `None`).
pub(crate) fn resolve_subdir_from_index(name: &str, index: &RegistryIndex) -> Option<String> {
    index
        .packages
        .iter()
        .find(|pkg| pkg.name == name)
        .and_then(|pkg| pkg.subdir.clone())
}

/// Fetch + parse the index, or `None` if it can't be reached/parsed. Used by
/// `install`/`update` so a registry outage degrades to the `turbo-*` default
/// instead of aborting the whole command.
pub(crate) fn load_index_quietly() -> Option<RegistryIndex> {
    let url = registry_index_url();
    let body = fetch_registry_index(&url).ok()?;
    parse_registry_index(&body).ok()
}

/// `turbolang search <query>` — fetch the published index, filter by query, and
/// print a table with install hints. Fails gracefully (no panic) when offline.
pub(crate) fn search_packages(query: &str) {
    let url = registry_index_url();
    let body = match fetch_registry_index(&url) {
        Ok(body) => body,
        Err(err) => {
            eprintln!("\x1b[1;31merror\x1b[0m: {err}");
            eprintln!("  The package index is fetched from {url}");
            eprintln!(
                "  Check your connection, or browse packages at https://turbolang.dev/packages"
            );
            std::process::exit(1);
        }
    };

    let index = match parse_registry_index(&body) {
        Ok(index) => index,
        Err(err) => {
            eprintln!("\x1b[1;31merror\x1b[0m: {err}");
            std::process::exit(1);
        }
    };

    let matches = filter_packages(&index, query);
    print_search_results(query, &matches, &mut std::io::stdout().lock());
}

/// Render search results as a table. Split out from [`search_packages`] so it
/// can be unit-tested against an in-memory writer.
pub(crate) fn print_search_results(
    query: &str,
    matches: &[&RegistryPackage],
    out: &mut impl Write,
) {
    if matches.is_empty() {
        if query.trim().is_empty() {
            let _ = writeln!(
                out,
                "The Turbo package index is empty — no packages published yet."
            );
        } else {
            let _ = writeln!(out, "No packages match \"{query}\".");
        }
        let _ = writeln!(
            out,
            "Publish yours with a PR: https://turbolang.dev/packages"
        );
        return;
    }

    let name_width = matches
        .iter()
        .map(|p| p.name.len())
        .max()
        .unwrap_or(4)
        .max(4);

    let _ = writeln!(
        out,
        "{:<name_width$}  DESCRIPTION",
        "NAME",
        name_width = name_width
    );
    for pkg in matches {
        let _ = writeln!(
            out,
            "{:<name_width$}  {}",
            pkg.name,
            pkg.description,
            name_width = name_width
        );
        let _ = writeln!(
            out,
            "{:<name_width$}  install: turbolang install {}",
            "",
            pkg.name,
            name_width = name_width
        );
    }

    let count = matches.len();
    let _ = writeln!(
        out,
        "\n{count} package{} found.",
        if count == 1 { "" } else { "s" }
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_index_json() -> &'static str {
        r#"{
          "schema_version": 1,
          "packages": [
            {
              "name": "turbo-json",
              "repo": "ZVN-DEV/monorepo",
              "description": "A fast JSON parser and serializer",
              "categories": ["serialization", "encoding"],
              "min_turbo_version": "0.10.0",
              "homepage": "https://example.com/turbo-json",
              "subdir": "packages/turbo-json"
            },
            {
              "name": "turbo-http",
              "repo": "ZVN-DEV/turbo-http",
              "description": "HTTP client and server primitives",
              "categories": ["web", "networking"]
            }
          ]
        }"#
    }

    #[test]
    fn parses_full_and_partial_entries() {
        let index = parse_registry_index(sample_index_json()).unwrap();
        assert_eq!(index.schema_version, 1);
        assert_eq!(index.packages.len(), 2);

        let json = &index.packages[0];
        assert_eq!(json.name, "turbo-json");
        assert_eq!(json.repo, "ZVN-DEV/monorepo");
        assert_eq!(json.categories, vec!["serialization", "encoding"]);
        assert_eq!(json.min_turbo_version.as_deref(), Some("0.10.0"));
        assert_eq!(
            json.homepage.as_deref(),
            Some("https://example.com/turbo-json")
        );
        assert_eq!(json.subdir.as_deref(), Some("packages/turbo-json"));

        let http = &index.packages[1];
        assert_eq!(http.min_turbo_version, None);
        assert_eq!(http.homepage, None);
        assert_eq!(http.subdir, None);
    }

    #[test]
    fn resolve_subdir_from_index_only_for_monorepo_entries() {
        let index = parse_registry_index(sample_index_json()).unwrap();
        assert_eq!(
            resolve_subdir_from_index("turbo-json", &index).as_deref(),
            Some("packages/turbo-json")
        );
        // A single-package repo entry has no subdir.
        assert_eq!(resolve_subdir_from_index("turbo-http", &index), None);
        assert_eq!(resolve_subdir_from_index("turbo-missing", &index), None);
    }

    #[test]
    fn parses_empty_seed_index() {
        let index =
            parse_registry_index(r#"{ "schema_version": 1, "_comment": "seed", "packages": [] }"#)
                .unwrap();
        assert_eq!(index.schema_version, 1);
        assert!(index.packages.is_empty());
    }

    #[test]
    fn rejects_malformed_json() {
        assert!(parse_registry_index("not json").is_err());
    }

    #[test]
    fn rejects_missing_packages_array() {
        assert!(parse_registry_index(r#"{ "schema_version": 1 }"#).is_err());
    }

    #[test]
    fn rejects_entry_missing_required_field() {
        let err =
            parse_registry_index(r#"{ "schema_version": 1, "packages": [ { "name": "x" } ] }"#)
                .unwrap_err();
        assert!(
            err.contains("repo") || err.contains("description"),
            "got: {err}"
        );
    }

    #[test]
    fn filter_matches_name_case_insensitively() {
        let index = parse_registry_index(sample_index_json()).unwrap();
        let matches = filter_packages(&index, "JSON");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].name, "turbo-json");
    }

    #[test]
    fn filter_matches_description() {
        let index = parse_registry_index(sample_index_json()).unwrap();
        let matches = filter_packages(&index, "serializer");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].name, "turbo-json");
    }

    #[test]
    fn filter_matches_category() {
        let index = parse_registry_index(sample_index_json()).unwrap();
        let matches = filter_packages(&index, "networking");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].name, "turbo-http");
    }

    #[test]
    fn empty_query_matches_all_sorted() {
        let index = parse_registry_index(sample_index_json()).unwrap();
        let matches = filter_packages(&index, "");
        assert_eq!(matches.len(), 2);
        // sorted by name: turbo-http before turbo-json
        assert_eq!(matches[0].name, "turbo-http");
        assert_eq!(matches[1].name, "turbo-json");
    }

    #[test]
    fn no_match_returns_empty() {
        let index = parse_registry_index(sample_index_json()).unwrap();
        assert!(filter_packages(&index, "nonexistent-xyz").is_empty());
    }

    #[test]
    fn resolve_repo_from_index_finds_exact_name() {
        let index = parse_registry_index(sample_index_json()).unwrap();
        assert_eq!(
            resolve_repo_from_index("turbo-http", &index).as_deref(),
            Some("ZVN-DEV/turbo-http")
        );
        assert_eq!(resolve_repo_from_index("turbo-missing", &index), None);
    }

    #[test]
    fn fetch_rejects_non_https_non_file_scheme() {
        let err = fetch_registry_index("http://insecure.example/index.json").unwrap_err();
        assert!(err.contains("only https:// and file://"), "got: {err}");
        let err = fetch_registry_index("ftp://example/index.json").unwrap_err();
        assert!(err.contains("only https:// and file://"), "got: {err}");
    }

    #[test]
    fn fetch_reads_local_file_url() {
        // Exercises the live fetch path against a local file:// fixture — no
        // network or curl needed, so this is hermetic in CI.
        let dir = std::env::temp_dir().join(format!(
            "turbo-registry-fixture-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("index.json");
        std::fs::write(&path, sample_index_json()).unwrap();

        let url = format!("file://{}", path.display());
        let body = fetch_registry_index(&url).unwrap();
        let index = parse_registry_index(&body).unwrap();
        let matches = filter_packages(&index, "json");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].name, "turbo-json");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn search_output_lists_matches_with_install_hint() {
        let index = parse_registry_index(sample_index_json()).unwrap();
        let matches = filter_packages(&index, "http");
        let mut buf: Vec<u8> = Vec::new();
        print_search_results("http", &matches, &mut buf);
        let out = String::from_utf8(buf).unwrap();
        assert!(out.contains("turbo-http"));
        assert!(out.contains("install: turbolang install turbo-http"));
        assert!(out.contains("1 package found."));
    }

    #[test]
    fn search_output_empty_state_points_to_publishing() {
        let index = RegistryIndex {
            schema_version: 1,
            packages: vec![],
        };
        let matches = filter_packages(&index, "");
        let mut buf: Vec<u8> = Vec::new();
        print_search_results("", &matches, &mut buf);
        let out = String::from_utf8(buf).unwrap();
        assert!(out.contains("empty"));
        assert!(out.contains("turbolang.dev/packages"));
    }
}
