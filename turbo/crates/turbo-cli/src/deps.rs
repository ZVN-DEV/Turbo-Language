//! Dependency resolution: manifest/lockfile parsing, git fetching, and the
//! `turbolang install` / `turbolang update` commands.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::diagnostics::io_reason;
use crate::project::extract_quoted_value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DependencySource {
    Path {
        path: String,
    },
    GitHub {
        repo: String,
        rev: Option<String>,
        version: Option<String>,
    },
    Version {
        version: String,
    },
    Unsupported {
        raw: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DependencySpec {
    name: String,
    section: String,
    source: DependencySource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LockedGitDependency {
    repo: String,
    rev: String,
}

pub(crate) fn parse_dependency_spec(name: &str, rest: &str, section: &str) -> DependencySpec {
    let source = if let Some(path) = extract_quoted_value(rest, "path") {
        DependencySource::Path { path }
    } else if let Some(repo) = extract_quoted_value(rest, "github") {
        let rev = extract_quoted_value(rest, "rev");
        let version = extract_quoted_value(rest, "version");
        DependencySource::GitHub { repo, rev, version }
    } else if let Some(version) = rest
        .trim()
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
    {
        DependencySource::Version {
            version: version.to_string(),
        }
    } else if let Some(version) = extract_quoted_value(rest, "version") {
        DependencySource::Version { version }
    } else {
        DependencySource::Unsupported {
            raw: rest.trim().to_string(),
        }
    };

    DependencySpec {
        name: name.trim().trim_matches('"').to_string(),
        section: section.to_string(),
        source,
    }
}

pub(crate) fn parse_dependencies_from_manifest(toml: &str) -> Vec<DependencySpec> {
    let mut current_section: Option<&str> = None;
    let mut deps = Vec::new();

    for raw_line in toml.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "[dependencies]" {
            current_section = Some("dependencies");
            continue;
        }
        if line == "[dev-dependencies]" {
            current_section = Some("dev-dependencies");
            continue;
        }
        if line.starts_with('[') {
            current_section = None;
            continue;
        }
        let Some(section) = current_section else {
            continue;
        };
        let Some((name, rest)) = line.split_once('=') else {
            continue;
        };
        deps.push(parse_dependency_spec(name, rest.trim(), section));
    }

    deps
}

pub(crate) fn parse_registry_map(toml: &str) -> HashMap<String, String> {
    let mut current_section: Option<&str> = None;
    let mut registries = HashMap::new();

    for raw_line in toml.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "[registries]" {
            current_section = Some("registries");
            continue;
        }
        if line.starts_with('[') {
            current_section = None;
            continue;
        }
        if current_section != Some("registries") {
            continue;
        }
        let Some((name, rest)) = line.split_once('=') else {
            continue;
        };
        let name = name.trim().trim_matches('"').to_string();
        let rest = rest.trim();
        if let Some(repo) = rest
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .map(|s| s.to_string())
        {
            registries.insert(name, repo);
        } else if let Some(repo) = extract_quoted_value(rest, "github") {
            registries.insert(name, repo);
        }
    }

    registries
}

pub(crate) fn default_registry_repo(name: &str) -> Option<String> {
    if name.starts_with("turbo-") {
        Some(format!("ZVN-DEV/{name}"))
    } else {
        None
    }
}

/// Resolve a bare package name to its `owner/repo`, consulting sources in
/// precedence order:
///   1. an explicit `[registries]` mapping in `turbo.toml` (project override),
///   2. the fetched registry index (`name` -> `repo`),
///   3. the `turbo-*` -> `ZVN-DEV/{name}` built-in default.
pub(crate) fn resolve_registry_repo_with_index(
    name: &str,
    registries: &HashMap<String, String>,
    index: Option<&crate::registry::RegistryIndex>,
) -> Option<String> {
    if let Some(repo) = registries.get(name).cloned() {
        return Some(repo);
    }
    if let Some(index) = index {
        if let Some(repo) = crate::registry::resolve_repo_from_index(name, index) {
            return Some(repo);
        }
    }
    default_registry_repo(name)
}

/// Whether any `version`-style dependency needs the registry index to resolve —
/// i.e. it has no explicit `[registries]` entry. When false, `install`/`update`
/// skip the network fetch entirely.
fn needs_registry_index(deps: &[DependencySpec], registries: &HashMap<String, String>) -> bool {
    deps.iter().any(|dep| {
        matches!(dep.source, DependencySource::Version { .. })
            && !registries.contains_key(&dep.name)
    })
}

pub(crate) fn validate_dependency_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("dependency name must not be empty".to_string());
    }
    if name.contains('/') || name.contains('\\') {
        return Err(format!(
            "dependency name `{name}` must not contain path separators"
        ));
    }
    if name == ".." || name.contains("..") {
        return Err(format!(
            "dependency name `{name}` must not contain `..` path traversal"
        ));
    }
    if Path::new(name).is_absolute() {
        return Err(format!(
            "dependency name `{name}` must not be an absolute path"
        ));
    }
    if is_windows_path_prefix(name) {
        return Err(format!(
            "dependency name `{name}` must not use a Windows path prefix"
        ));
    }

    Ok(())
}

pub(crate) fn is_windows_path_prefix(name: &str) -> bool {
    let bytes = name.as_bytes();
    bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic()
}

pub(crate) fn dependency_target_path(
    modules_dir: &Path,
    dep_name: &str,
) -> Result<PathBuf, String> {
    validate_dependency_name(dep_name)?;
    let modules_dir = canonical_modules_dir(modules_dir)?;
    let target = modules_dir.join(dep_name);
    if !target.starts_with(&modules_dir) {
        return Err(format!(
            "dependency target `{}` would escape `{}`",
            target.display(),
            modules_dir.display()
        ));
    }

    Ok(target)
}

pub(crate) fn canonical_modules_dir(modules_dir: &Path) -> Result<PathBuf, String> {
    let cwd = std::fs::canonicalize(".")
        .map_err(|e| format!("could not resolve current directory for turbo_modules: {e}"))?;

    if let Ok(metadata) = std::fs::symlink_metadata(modules_dir) {
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "turbo_modules directory `{}` must not be a symlink",
                modules_dir.display()
            ));
        }
    }

    let resolved = if modules_dir.exists() {
        std::fs::canonicalize(modules_dir).map_err(|e| {
            format!(
                "could not resolve turbo_modules directory `{}`: {e}",
                modules_dir.display()
            )
        })?
    } else if modules_dir.is_absolute() {
        let parent = modules_dir.parent().ok_or_else(|| {
            format!(
                "could not resolve turbo_modules directory `{}`: missing parent",
                modules_dir.display()
            )
        })?;
        let name = modules_dir.file_name().ok_or_else(|| {
            format!(
                "could not resolve turbo_modules directory `{}`: missing directory name",
                modules_dir.display()
            )
        })?;
        std::fs::canonicalize(parent)
            .map(|parent| parent.join(name))
            .map_err(|e| {
                format!(
                    "could not resolve turbo_modules parent `{}`: {e}",
                    parent.display()
                )
            })?
    } else {
        cwd.join(modules_dir)
    };

    if !resolved.starts_with(&cwd) {
        return Err(format!(
            "turbo_modules directory `{}` must stay inside project root `{}`",
            resolved.display(),
            cwd.display()
        ));
    }

    Ok(resolved)
}

pub(crate) fn validate_manifest_dependency_names(deps: &[DependencySpec]) {
    for dep in deps {
        if let Err(err) = validate_dependency_name(&dep.name) {
            eprintln!(
                "\x1b[1;31merror\x1b[0m: invalid dependency name `{}` in [{}]: {}",
                dep.name, dep.section, err
            );
            std::process::exit(1);
        }
    }
}

pub(crate) fn read_lockfile() -> HashMap<String, LockedGitDependency> {
    let contents = match std::fs::read_to_string("turbo.lock") {
        Ok(s) => s,
        Err(_) => return HashMap::new(),
    };

    let mut current_section = None;
    let mut locks = HashMap::new();

    for raw_line in contents.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "[github]" {
            current_section = Some("github");
            continue;
        }
        if line.starts_with('[') {
            current_section = None;
            continue;
        }
        if current_section != Some("github") {
            continue;
        }
        let Some((name, rest)) = line.split_once('=') else {
            continue;
        };
        let Some(value) = rest
            .trim()
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
        else {
            continue;
        };
        let Some((repo, rev)) = value.rsplit_once('#') else {
            continue;
        };
        locks.insert(
            name.trim().to_string(),
            LockedGitDependency {
                repo: repo.to_string(),
                rev: rev.to_string(),
            },
        );
    }

    locks
}

pub(crate) fn write_lockfile(locks: &HashMap<String, LockedGitDependency>) {
    if locks.is_empty() {
        let _ = std::fs::remove_file("turbo.lock");
        return;
    }

    let mut names: Vec<&String> = locks.keys().collect();
    names.sort();

    let mut out = String::from(
        "# This file is generated by `turbolang install` / `turbolang update`.\n\
         # It pins GitHub dependencies to exact commits for reproducible installs.\n\n\
         [github]\n",
    );
    for name in names {
        let entry = &locks[name];
        out.push_str(&format!("{name} = \"{}#{}\"\n", entry.repo, entry.rev));
    }

    if let Err(e) = std::fs::write("turbo.lock", out) {
        eprintln!(
            "\x1b[1;31merror\x1b[0m: could not write turbo.lock: {}",
            io_reason(&e)
        );
        std::process::exit(1);
    }
}

pub(crate) fn git_output(args: &[&str]) -> Result<String, String> {
    let output = std::process::Command::new("git").args(args).output();
    match output {
        Ok(o) if o.status.success() => Ok(String::from_utf8_lossy(&o.stdout).trim().to_string()),
        Ok(o) => Err(String::from_utf8_lossy(&o.stderr).trim().to_string()),
        Err(e) => Err(e.to_string()),
    }
}

pub(crate) fn git_output_in_dir(dir: &Path, args: &[&str]) -> Result<String, String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output();
    match output {
        Ok(o) if o.status.success() => Ok(String::from_utf8_lossy(&o.stdout).trim().to_string()),
        Ok(o) => Err(String::from_utf8_lossy(&o.stderr).trim().to_string()),
        Err(e) => Err(e.to_string()),
    }
}

pub(crate) fn current_git_head(dir: &Path) -> Result<String, String> {
    git_output_in_dir(dir, &["rev-parse", "HEAD"])
}

pub(crate) fn clone_github_repo(repo: &str, target: &Path) -> Result<(), String> {
    let url = format!("https://github.com/{repo}.git");
    let target_str = target.to_string_lossy().to_string();
    git_output(&["clone", "--depth=1", &url, &target_str]).map(|_| ())
}

pub(crate) fn checkout_git_rev(dir: &Path, rev: &str) -> Result<(), String> {
    git_output_in_dir(dir, &["fetch", "--depth=1", "origin", rev])?;
    git_output_in_dir(dir, &["checkout", "--detach", rev]).map(|_| ())
}

pub(crate) fn git_ls_remote_tags(repo: &str) -> Result<Vec<(String, String)>, String> {
    let url = format!("https://github.com/{repo}.git");
    let output = git_output(&["ls-remote", "--tags", &url])?;
    let mut tags = HashMap::new();

    for line in output.lines() {
        let Some((sha, raw_ref)) = line.split_once('\t') else {
            continue;
        };
        let Some(tag_ref) = raw_ref.strip_prefix("refs/tags/") else {
            continue;
        };
        let tag = tag_ref.strip_suffix("^{}").unwrap_or(tag_ref).to_string();
        tags.insert(tag, sha.to_string());
    }

    let mut out: Vec<(String, String)> = tags.into_iter().collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

pub(crate) fn parse_semver_like(input: &str) -> Option<(u64, u64, u64)> {
    let trimmed = input.trim().trim_start_matches('v');
    let mut parts = trimmed.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next().map_or(Some(0), |p| p.parse().ok())?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

pub(crate) fn select_tag_for_version(
    version: &str,
    tags: &[(String, String)],
) -> Option<(String, String)> {
    let requested = parse_semver_like(version)?;
    let exact_requested = version.trim_start_matches('v').split('.').count() >= 3;

    let mut exact = None;
    let mut matching_minor = Vec::new();

    for (tag, sha) in tags {
        let parsed = match parse_semver_like(tag) {
            Some(v) => v,
            None => continue,
        };
        if exact_requested {
            if parsed == requested {
                exact = Some((tag.clone(), sha.clone()));
                break;
            }
        } else if parsed.0 == requested.0 && parsed.1 == requested.1 {
            matching_minor.push((parsed, tag.clone(), sha.clone()));
        }
    }

    if let Some(found) = exact {
        return Some(found);
    }

    if matching_minor.is_empty() {
        return None;
    }
    matching_minor.sort_by_key(|a| a.0);
    let (_, tag, sha) = matching_minor.pop()?;
    Some((tag, sha))
}

pub(crate) fn resolve_versioned_rev(repo: &str, version: &str) -> Result<(String, String), String> {
    let tags = git_ls_remote_tags(repo)?;
    select_tag_for_version(version, &tags)
        .ok_or_else(|| format!("no tag found in {repo} matching version {version}"))
}

/// Install dependencies listed in `turbo.toml` by symlinking path dependencies
/// into a local `turbo_modules/` directory.
pub(crate) fn install_deps() {
    let toml = std::fs::read_to_string("turbo.toml").unwrap_or_else(|_| {
        eprintln!("\x1b[1;31merror\x1b[0m: no turbo.toml found in current directory");
        std::process::exit(1);
    });

    std::fs::create_dir_all("turbo_modules").ok();
    let deps = parse_dependencies_from_manifest(&toml);
    validate_manifest_dependency_names(&deps);
    let registries = parse_registry_map(&toml);
    // Consult the published registry index for `name -> repo` mappings, but only
    // when a bare `version` dependency lacks an explicit `[registries]` entry.
    // A registry outage degrades to the `turbo-*` default rather than aborting.
    // (Not cached to disk: the CLI has no cache dir today, and the index is
    // fetched at most once per command regardless of dependency count.)
    let index = if needs_registry_index(&deps, &registries) {
        crate::registry::load_index_quietly()
    } else {
        None
    };
    let mut lockfile = read_lockfile();
    let mut count = 0u32;
    let mut unsupported = Vec::new();

    for dep in deps {
        match dep.source {
            DependencySource::Path { path } => {
                let source_path = std::path::Path::new(&path);
                let canonical = match std::fs::canonicalize(source_path) {
                    Ok(p) => p,
                    Err(e) => {
                        eprintln!(
                            "\x1b[1;31merror\x1b[0m: could not resolve dependency path `{}`: {}",
                            path,
                            io_reason(&e)
                        );
                        std::process::exit(1);
                    }
                };

                let target = dependency_target_path(Path::new("turbo_modules"), &dep.name)
                    .unwrap_or_else(|err| {
                        eprintln!("\x1b[1;31merror\x1b[0m: {err}");
                        std::process::exit(1);
                    });
                if target.exists() {
                    // Remove existing symlink or directory
                    if target.is_dir() {
                        std::fs::remove_dir_all(&target).ok();
                    } else {
                        std::fs::remove_file(&target).ok();
                    }
                }

                #[cfg(unix)]
                {
                    std::os::unix::fs::symlink(&canonical, &target).unwrap_or_else(|e| {
                        eprintln!(
                            "\x1b[1;31merror\x1b[0m: could not create symlink for `{}`: {}",
                            dep.name,
                            io_reason(&e)
                        );
                        std::process::exit(1);
                    });
                }

                #[cfg(not(unix))]
                {
                    // On non-Unix platforms, fall back to copying
                    fn copy_dir_recursive(
                        src: &std::path::Path,
                        dst: &std::path::Path,
                    ) -> std::io::Result<()> {
                        std::fs::create_dir_all(dst)?;
                        for entry in std::fs::read_dir(src)? {
                            let entry = entry?;
                            let ty = entry.file_type()?;
                            let dest = dst.join(entry.file_name());
                            if ty.is_dir() {
                                copy_dir_recursive(&entry.path(), &dest)?;
                            } else {
                                std::fs::copy(entry.path(), dest)?;
                            }
                        }
                        Ok(())
                    }
                    copy_dir_recursive(&canonical, &target).unwrap_or_else(|e| {
                        eprintln!(
                            "\x1b[1;31merror\x1b[0m: could not copy dependency `{}`: {}",
                            dep.name,
                            io_reason(&e)
                        );
                        std::process::exit(1);
                    });
                }

                eprintln!(
                    "  \x1b[32m\u{2713}\x1b[0m Installed {} -> {} ({})",
                    dep.name, path, dep.section
                );
                count += 1;
            }
            DependencySource::GitHub { repo, rev, version } => {
                let target = dependency_target_path(Path::new("turbo_modules"), &dep.name)
                    .unwrap_or_else(|err| {
                        eprintln!("\x1b[1;31merror\x1b[0m: {err}");
                        std::process::exit(1);
                    });
                let resolved = if let Some(wanted_rev) = rev.clone() {
                    Ok((wanted_rev.clone(), format!("rev {wanted_rev}")))
                } else if let Some(version) = version.as_deref() {
                    resolve_versioned_rev(&repo, version)
                        .map(|(tag, sha)| (sha, format!("tag {tag}")))
                } else if let Some(locked) = lockfile
                    .get(&dep.name)
                    .filter(|entry| entry.repo == repo)
                    .map(|entry| entry.rev.clone())
                {
                    Ok((locked, "lockfile".to_string()))
                } else {
                    Err("no rev, version, or existing turbo.lock entry".to_string())
                };
                let (pinned_rev, pinned_label) = match resolved {
                    Ok(v) => v,
                    Err(err) => {
                        eprintln!(
                            "  \x1b[31m\u{2717}\x1b[0m Failed to resolve {} from github:{}: {}",
                            dep.name, repo, err
                        );
                        continue;
                    }
                };

                if target.exists() {
                    if current_git_head(&target).ok().as_deref() != Some(pinned_rev.as_str()) {
                        if let Err(err) = checkout_git_rev(&target, &pinned_rev) {
                            eprintln!(
                                "  \x1b[31m\u{2717}\x1b[0m Failed to pin {} to {}: {}",
                                dep.name, pinned_rev, err
                            );
                            continue;
                        }
                    }
                } else {
                    eprintln!(
                        "  \x1b[36m\u{2193}\x1b[0m Cloning {} from github:{}...",
                        dep.name, repo
                    );
                    if let Err(err) = clone_github_repo(&repo, &target) {
                        eprintln!(
                            "  \x1b[31m\u{2717}\x1b[0m Failed to clone {}: {}",
                            dep.name, err
                        );
                        continue;
                    }
                    if let Err(err) = checkout_git_rev(&target, &pinned_rev) {
                        eprintln!(
                            "  \x1b[31m\u{2717}\x1b[0m Failed to pin {} to {}: {}",
                            dep.name, pinned_rev, err
                        );
                        continue;
                    }
                }

                match current_git_head(&target) {
                    Ok(head) => {
                        lockfile.insert(
                            dep.name.clone(),
                            LockedGitDependency {
                                repo: repo.clone(),
                                rev: head.clone(),
                            },
                        );
                        eprintln!(
                            "  \x1b[32m\u{2713}\x1b[0m Installed {} from github:{} @ {} via {} ({})",
                            dep.name,
                            repo,
                            &head[..head.len().min(12)],
                            pinned_label,
                            dep.section
                        );
                        count += 1;
                    }
                    Err(err) => eprintln!(
                        "  \x1b[31m\u{2717}\x1b[0m Failed to resolve installed rev for {}: {}",
                        dep.name, err
                    ),
                }
            }
            DependencySource::Version { version } => {
                let Some(repo) =
                    resolve_registry_repo_with_index(&dep.name, &registries, index.as_ref())
                else {
                    eprintln!(
                        "  \x1b[31m\u{2717}\x1b[0m No registry mapping found for {} {}",
                        dep.name, version
                    );
                    continue;
                };
                let target = dependency_target_path(Path::new("turbo_modules"), &dep.name)
                    .unwrap_or_else(|err| {
                        eprintln!("\x1b[1;31merror\x1b[0m: {err}");
                        std::process::exit(1);
                    });
                let (tag, pinned_rev) = match resolve_versioned_rev(&repo, &version) {
                    Ok(v) => v,
                    Err(err) => {
                        eprintln!(
                            "  \x1b[31m\u{2717}\x1b[0m Failed to resolve {} {}: {}",
                            dep.name, version, err
                        );
                        continue;
                    }
                };
                if target.exists() {
                    if current_git_head(&target).ok().as_deref() != Some(pinned_rev.as_str()) {
                        if let Err(err) = checkout_git_rev(&target, &pinned_rev) {
                            eprintln!(
                                "  \x1b[31m\u{2717}\x1b[0m Failed to pin {} to {}: {}",
                                dep.name, pinned_rev, err
                            );
                            continue;
                        }
                    }
                } else {
                    eprintln!(
                        "  \x1b[36m\u{2193}\x1b[0m Cloning {} {} from {}...",
                        dep.name, version, repo
                    );
                    if let Err(err) = clone_github_repo(&repo, &target) {
                        eprintln!(
                            "  \x1b[31m\u{2717}\x1b[0m Failed to clone {}: {}",
                            dep.name, err
                        );
                        continue;
                    }
                    if let Err(err) = checkout_git_rev(&target, &pinned_rev) {
                        eprintln!(
                            "  \x1b[31m\u{2717}\x1b[0m Failed to pin {} to {}: {}",
                            dep.name, pinned_rev, err
                        );
                        continue;
                    }
                }
                lockfile.insert(
                    dep.name.clone(),
                    LockedGitDependency {
                        repo: repo.clone(),
                        rev: pinned_rev.clone(),
                    },
                );
                eprintln!(
                    "  \x1b[32m\u{2713}\x1b[0m Installed {} {} from {} @ {} via {} ({})",
                    dep.name,
                    version,
                    repo,
                    &pinned_rev[..pinned_rev.len().min(12)],
                    tag,
                    dep.section
                );
                count += 1;
            }
            DependencySource::Unsupported { raw } => unsupported.push((dep.name, dep.section, raw)),
        }
    }

    if !unsupported.is_empty() {
        for (name, section, raw) in &unsupported {
            eprintln!(
                "  \x1b[31m\u{2717}\x1b[0m Unsupported dependency format for {} ({}) -> {}",
                name, section, raw
            );
        }
        eprintln!("\nerror: unsupported dependency syntax in turbo.toml.");
        std::process::exit(1);
    }

    write_lockfile(&lockfile);

    if count == 0 {
        eprintln!("No dependencies found in turbo.toml");
    } else {
        eprintln!(
            "Installed {} dependenc{}.",
            count,
            if count == 1 { "y" } else { "ies" }
        );
    }
}

/// Update all GitHub dependencies to their latest versions by pulling in each cloned repo.
pub(crate) fn update_deps() {
    let toml = std::fs::read_to_string("turbo.toml").unwrap_or_else(|_| {
        eprintln!("\x1b[1;31merror\x1b[0m: no turbo.toml found in current directory");
        std::process::exit(1);
    });

    let deps = parse_dependencies_from_manifest(&toml);
    validate_manifest_dependency_names(&deps);
    let registries = parse_registry_map(&toml);
    let index = if needs_registry_index(&deps, &registries) {
        crate::registry::load_index_quietly()
    } else {
        None
    };
    let mut lockfile = read_lockfile();
    let mut count = 0u32;
    let mut unsupported = Vec::new();

    for dep in deps {
        match dep.source {
            DependencySource::GitHub { repo, rev, version } => {
                let target = dependency_target_path(Path::new("turbo_modules"), &dep.name)
                    .unwrap_or_else(|err| {
                        eprintln!("\x1b[1;31merror\x1b[0m: {err}");
                        std::process::exit(1);
                    });
                if !target.exists() {
                    eprintln!(
                        "  \x1b[33m!\x1b[0m {} not installed — run `turbolang install` first",
                        dep.name
                    );
                    continue;
                }

                if let Some(wanted) = rev.as_deref() {
                    match checkout_git_rev(&target, wanted).and_then(|_| current_git_head(&target))
                    {
                        Ok(head) => {
                            lockfile.insert(
                                dep.name.clone(),
                                LockedGitDependency {
                                    repo: repo.clone(),
                                    rev: head.clone(),
                                },
                            );
                            eprintln!(
                                "  \x1b[32m\u{2713}\x1b[0m {} pinned at manifest rev {}",
                                dep.name,
                                &head[..head.len().min(12)]
                            );
                            count += 1;
                        }
                        Err(err) => eprintln!(
                            "  \x1b[31m\u{2717}\x1b[0m Failed to pin {}: {}",
                            dep.name, err
                        ),
                    }
                    continue;
                }

                if let Some(version) = version.as_deref() {
                    match resolve_versioned_rev(&repo, version)
                        .and_then(|(tag, rev)| checkout_git_rev(&target, &rev).map(|_| (tag, rev)))
                        .and_then(|(tag, rev)| {
                            current_git_head(&target).map(|head| (tag, rev, head))
                        }) {
                        Ok((tag, _rev, head)) => {
                            lockfile.insert(
                                dep.name.clone(),
                                LockedGitDependency {
                                    repo: repo.clone(),
                                    rev: head.clone(),
                                },
                            );
                            eprintln!(
                                "  \x1b[32m\u{2713}\x1b[0m {} updated to {} ({})",
                                dep.name,
                                &head[..head.len().min(12)],
                                tag
                            );
                            count += 1;
                        }
                        Err(err) => eprintln!(
                            "  \x1b[31m\u{2717}\x1b[0m Failed to update {} {}: {}",
                            dep.name, version, err
                        ),
                    }
                    continue;
                }

                eprintln!(
                    "  \x1b[36m\u{2193}\x1b[0m Updating {} from github:{}...",
                    dep.name, repo
                );
                match git_output_in_dir(&target, &["pull", "--ff-only"])
                    .and_then(|_| current_git_head(&target))
                {
                    Ok(head) => {
                        lockfile.insert(
                            dep.name.clone(),
                            LockedGitDependency {
                                repo: repo.clone(),
                                rev: head.clone(),
                            },
                        );
                        eprintln!(
                            "  \x1b[32m\u{2713}\x1b[0m Updated {} -> {}",
                            dep.name,
                            &head[..head.len().min(12)]
                        );
                        count += 1;
                    }
                    Err(err) => eprintln!(
                        "  \x1b[31m\u{2717}\x1b[0m Failed to update {}: {}",
                        dep.name, err
                    ),
                }
            }
            DependencySource::Version { version } => {
                let Some(repo) =
                    resolve_registry_repo_with_index(&dep.name, &registries, index.as_ref())
                else {
                    eprintln!(
                        "  \x1b[31m\u{2717}\x1b[0m No registry mapping found for {} {}",
                        dep.name, version
                    );
                    continue;
                };
                let target = dependency_target_path(Path::new("turbo_modules"), &dep.name)
                    .unwrap_or_else(|err| {
                        eprintln!("\x1b[1;31merror\x1b[0m: {err}");
                        std::process::exit(1);
                    });
                if !target.exists() {
                    eprintln!(
                        "  \x1b[33m!\x1b[0m {} not installed — run `turbolang install` first",
                        dep.name
                    );
                    continue;
                }
                match resolve_versioned_rev(&repo, &version)
                    .and_then(|(tag, rev)| checkout_git_rev(&target, &rev).map(|_| (tag, rev)))
                    .and_then(|(tag, rev)| current_git_head(&target).map(|head| (tag, rev, head)))
                {
                    Ok((tag, _rev, head)) => {
                        lockfile.insert(
                            dep.name.clone(),
                            LockedGitDependency {
                                repo: repo.clone(),
                                rev: head.clone(),
                            },
                        );
                        eprintln!(
                            "  \x1b[32m\u{2713}\x1b[0m {} {} -> {} ({})",
                            dep.name,
                            version,
                            &head[..head.len().min(12)],
                            tag
                        );
                        count += 1;
                    }
                    Err(err) => eprintln!(
                        "  \x1b[31m\u{2717}\x1b[0m Failed to update {} {}: {}",
                        dep.name, version, err
                    ),
                }
            }
            DependencySource::Unsupported { raw } => unsupported.push((dep.name, dep.section, raw)),
            DependencySource::Path { .. } => {}
        }
    }

    if !unsupported.is_empty() {
        for (name, section, raw) in &unsupported {
            eprintln!(
                "  \x1b[31m\u{2717}\x1b[0m Unsupported dependency format for {} ({}) -> {}",
                name, section, raw
            );
        }
        std::process::exit(1);
    }

    write_lockfile(&lockfile);

    if count == 0 {
        eprintln!("No GitHub dependencies found to update.");
    } else {
        eprintln!(
            "Updated {} dependenc{}.",
            count,
            if count == 1 { "y" } else { "ies" }
        );
    }
}

#[cfg(test)]
mod dependency_tests {
    use super::*;

    #[test]
    fn parse_registry_section() {
        let toml = r#"
[registries]
turbo-db = "ZVN-DEV/turbo-db"
turbo-test-utils = { github = "ZVN-DEV/turbo-test-utils" }
"#;
        let registries = parse_registry_map(toml);
        assert_eq!(
            registries.get("turbo-db").map(String::as_str),
            Some("ZVN-DEV/turbo-db")
        );
        assert_eq!(
            registries.get("turbo-test-utils").map(String::as_str),
            Some("ZVN-DEV/turbo-test-utils")
        );
    }

    fn index_with(name: &str, repo: &str) -> crate::registry::RegistryIndex {
        crate::registry::parse_registry_index(&format!(
            r#"{{ "schema_version": 1, "packages": [ {{ "name": "{name}", "repo": "{repo}", "description": "d" }} ] }}"#
        ))
        .unwrap()
    }

    #[test]
    fn resolve_prefers_explicit_registry_over_index_and_default() {
        let mut registries = HashMap::new();
        registries.insert("turbo-db".to_string(), "explicit/turbo-db".to_string());
        let index = index_with("turbo-db", "indexed/turbo-db");
        assert_eq!(
            resolve_registry_repo_with_index("turbo-db", &registries, Some(&index)).as_deref(),
            Some("explicit/turbo-db")
        );
    }

    #[test]
    fn resolve_uses_index_when_no_explicit_mapping() {
        let registries = HashMap::new();
        // A name that would NOT match the `turbo-*` default, proving the index
        // is what resolved it.
        let index = index_with("cool-lib", "someone/cool-lib");
        assert_eq!(
            resolve_registry_repo_with_index("cool-lib", &registries, Some(&index)).as_deref(),
            Some("someone/cool-lib")
        );
    }

    #[test]
    fn resolve_falls_back_to_turbo_default_without_index() {
        let registries = HashMap::new();
        assert_eq!(
            resolve_registry_repo_with_index("turbo-http", &registries, None).as_deref(),
            Some("ZVN-DEV/turbo-http")
        );
        // A non-`turbo-*` name with no mapping and no index resolves to nothing.
        assert_eq!(
            resolve_registry_repo_with_index("cool-lib", &registries, None),
            None
        );
    }

    #[test]
    fn needs_index_only_for_unmapped_version_deps() {
        let deps = parse_dependencies_from_manifest(
            r#"
[dependencies]
turbo-db = "0.1"
"#,
        );
        let empty = HashMap::new();
        assert!(needs_registry_index(&deps, &empty));

        let mut mapped = HashMap::new();
        mapped.insert("turbo-db".to_string(), "ZVN-DEV/turbo-db".to_string());
        assert!(!needs_registry_index(&deps, &mapped));

        // Path/GitHub deps never need the index.
        let path_deps = parse_dependencies_from_manifest(
            r#"
[dependencies]
local = { path = "../local" }
"#,
        );
        assert!(!needs_registry_index(&path_deps, &empty));
    }

    #[test]
    fn parse_version_dependency() {
        let deps = parse_dependencies_from_manifest(
            r#"
[dependencies]
turbo-db = "0.1"
agent-kit = { github = "owner/agent-kit", version = "1.2" }
"#,
        );
        assert!(matches!(
            deps[0].source,
            DependencySource::Version { ref version } if version == "0.1"
        ));
        assert!(matches!(
            deps[1].source,
            DependencySource::GitHub { ref repo, version: Some(ref version), .. }
                if repo == "owner/agent-kit" && version == "1.2"
        ));
    }

    #[test]
    fn validate_dependency_name_accepts_plain_package_names() {
        for name in ["turbo-db", "agent_kit", "http2"] {
            assert!(
                validate_dependency_name(name).is_ok(),
                "{name} should be a valid dependency name"
            );
        }
    }

    #[test]
    fn validate_dependency_name_rejects_path_like_names() {
        for name in [
            "",
            "..",
            "../escape",
            "escape/child",
            "escape\\child",
            "/absolute",
            "C:escape",
            "C:\\escape",
            "pkg..escape",
        ] {
            assert!(
                validate_dependency_name(name).is_err(),
                "{name:?} should be rejected as a dependency name"
            );
        }
    }

    #[test]
    fn dependency_target_path_stays_under_turbo_modules() {
        let test_root = PathBuf::from(format!(
            ".turbo-cli-dep-target-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let modules_dir = test_root.join("turbo_modules");
        std::fs::create_dir_all(&modules_dir).unwrap();

        let target = dependency_target_path(&modules_dir, "safe_dep").unwrap();
        let canonical_modules = std::fs::canonicalize(&modules_dir).unwrap();
        assert!(target.starts_with(&canonical_modules));
        assert_eq!(target, canonical_modules.join("safe_dep"));

        let err = dependency_target_path(&modules_dir, "../escape").unwrap_err();
        assert!(
            err.contains("path separators") || err.contains("path traversal"),
            "unexpected error: {err}"
        );

        std::fs::remove_dir_all(&test_root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn dependency_target_path_rejects_symlinked_turbo_modules() {
        let test_root = PathBuf::from(format!(
            ".turbo-cli-dep-target-symlink-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let outside = test_root.join("outside");
        let modules_dir = test_root.join("turbo_modules");
        std::fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, &modules_dir).unwrap();

        let err = dependency_target_path(&modules_dir, "safe_dep").unwrap_err();
        assert!(
            err.contains("must not be a symlink"),
            "unexpected error: {err}"
        );

        std::fs::remove_dir_all(&test_root).unwrap();
    }

    #[test]
    fn select_latest_patch_for_minor_version() {
        let tags = vec![
            ("v0.1.0".to_string(), "aaa".to_string()),
            ("v0.1.4".to_string(), "bbb".to_string()),
            ("v0.2.0".to_string(), "ccc".to_string()),
        ];
        let selected = select_tag_for_version("0.1", &tags).unwrap();
        assert_eq!(selected.0, "v0.1.4");
        assert_eq!(selected.1, "bbb");
    }

    #[test]
    fn select_exact_patch_when_requested() {
        let tags = vec![
            ("v0.1.0".to_string(), "aaa".to_string()),
            ("v0.1.4".to_string(), "bbb".to_string()),
        ];
        let selected = select_tag_for_version("0.1.0", &tags).unwrap();
        assert_eq!(selected.0, "v0.1.0");
    }
}
