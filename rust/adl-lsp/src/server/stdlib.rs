//! The ADL standard library (`sys.*` and `adlc.*` modules), embedded in the binary.
//!
//! The ADL compiler ships these modules alongside itself, so real workspaces import
//! `sys.types` and friends without having the files anywhere in their own tree. The language
//! server needs real files to resolve imports and to give goto-definition somewhere to land.
//!
//! [`locate`] finds the copy installed with the user's toolchain. When there is none, the
//! embedded copy is written to a per-version cache directory. Either way the result is
//! registered as a package root of last resort (see
//! [`packages::resolve_import`](super::packages::resolve_import)).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{OnceLock, RwLock};
use std::time::{Duration, Instant};

use tracing::{debug, info, warn};

macro_rules! stdlib_file {
    ($path:literal) => {
        ($path, include_str!(concat!("../../stdlib/", $path)))
    };
}

/// `(path relative to the package root, contents)` for every standard library file.
const FILES: &[(&str, &str)] = &[
    stdlib_file!("adlc/config/cpp.adl"),
    stdlib_file!("adlc/config/haskell.adl"),
    stdlib_file!("adlc/config/java.adl"),
    stdlib_file!("adlc/config/rust.adl"),
    stdlib_file!("adlc/config/typescript.adl"),
    stdlib_file!("adlc/package.adl"),
    stdlib_file!("sys/adlast.adl"),
    stdlib_file!("sys/adlast.adl-java"),
    stdlib_file!("sys/annotations.adl"),
    stdlib_file!("sys/dynamic.adl"),
    stdlib_file!("sys/dynamic.adl-java"),
    stdlib_file!("sys/types.adl"),
    stdlib_file!("sys/types.adl-cpp"),
    stdlib_file!("sys/types.adl-hs"),
    stdlib_file!("sys/types.adl-java"),
    stdlib_file!("sys/types.adl-rs"),
    stdlib_file!("LICENSE"),
];

/// Marks the materialized directory as a package root so files inside it are never attributed
/// to a marker further up the filesystem.
const PACKAGE_MARKER: &str = "{\n  \"name\": \"adl-stdlib\",\n  \"dependencies\": []\n}\n";

/// The standard library root in use, set by [`activate`].
static ACTIVE_ROOT: RwLock<Option<PathBuf>> = RwLock::new(None);

/// Where a standard library was found, in priority order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// `--stdlib-dir` / `ADL_LSP_STDLIB_DIR` / the `adl.stdlibDir` editor setting
    Configured,
    /// `$ADL_ROOT/lib/adl`, the override the compiler itself honours
    AdlRoot,
    /// A toolchain installed inside the workspace, e.g. `<repo>/.local/lib/adl`
    Workspace,
    /// `../lib/adl` relative to the `adlc` executable found on `PATH`
    CompilerOnPath,
    /// The answer from running `adlc show --adlstdlib`
    CompilerQuery,
    /// A per-user install, e.g. `~/.cache/adl/<version>` or a `proto` managed toolchain
    UserInstall,
    /// The copy embedded in this binary
    Embedded,
}

impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Source::Configured => "configured stdlib dir",
            Source::AdlRoot => "ADL_ROOT",
            Source::Workspace => "toolchain installed in the workspace",
            Source::CompilerOnPath => "adlc on PATH",
            Source::CompilerQuery => "adlc show --adlstdlib",
            Source::UserInstall => "per-user adlc install",
            Source::Embedded => "embedded copy",
        })
    }
}

/// Everything [`locate`] consults. Built from the process environment by
/// [`Locations::from_environment`]; tests construct it directly.
#[derive(Debug, Default, Clone)]
pub struct Locations {
    /// An explicitly configured standard library directory.
    pub configured: Option<PathBuf>,
    /// The value of `$ADL_ROOT`.
    pub adl_root: Option<PathBuf>,
    /// The workspace's ADL search dirs; toolchains installed near them are considered.
    pub search_dirs: Vec<PathBuf>,
    /// The directories on `$PATH`.
    pub path: Vec<PathBuf>,
    /// Directories holding one install per version, as `<dir>/<version>/lib/adl`.
    pub install_dirs: Vec<PathBuf>,
    /// Whether to run `adlc show --adlstdlib` when `adlc` is on `PATH` but its files are not
    /// next to it (shims, cabal and nix installs).
    pub query_compiler: bool,
}

impl Locations {
    pub fn from_environment(configured: Option<PathBuf>, search_dirs: &[PathBuf]) -> Self {
        let home = env_dir("HOME").or_else(|| env_dir("USERPROFILE"));
        let in_home = |relative: &str| home.as_ref().map(|home| home.join(relative));

        // The wrapper script in the ADL install docs unpacks releases into a cache directory;
        // `proto` unpacks the same archives under its own tools directory.
        let install_dirs = [
            env_dir("XDG_CACHE_HOME").map(|dir| dir.join("adl")),
            in_home("Library/Caches/adl"),
            in_home(".cache/adl"),
            env_dir("LOCALAPPDATA").map(|dir| dir.join("adl")),
            env_dir("PROTO_HOME").map(|dir| dir.join("tools/adlc")),
            in_home(".proto/tools/adlc"),
            in_home(".config/proto/tools/adlc"),
        ]
        .into_iter()
        .flatten()
        .collect();

        Self {
            configured,
            adl_root: env_dir("ADL_ROOT"),
            search_dirs: search_dirs.to_vec(),
            path: std::env::var_os("PATH")
                .map(|path| std::env::split_paths(&path).collect())
                .unwrap_or_default(),
            install_dirs,
            query_compiler: true,
        }
    }
}

/// Find the standard library installed with the user's ADL toolchain.
///
/// Mirrors how `adlc` locates its own files (`$ADL_ROOT`, then relative to the executable,
/// then its compiled-in data directory) and adds the places toolchains are commonly unpacked.
/// Returns `None` when no install is found, in which case the embedded copy is used.
pub fn locate(locations: &Locations) -> Option<(PathBuf, Source)> {
    if let Some(configured) = &locations.configured {
        if is_stdlib_dir(configured) {
            return Some((configured.clone(), Source::Configured));
        }
        warn!(
            "configured stdlib dir {} does not contain sys/types.adl, ignoring it",
            configured.display()
        );
    }

    if let Some(adl_root) = &locations.adl_root {
        let dir = adl_root.join("lib").join("adl");
        if is_stdlib_dir(&dir) {
            return Some((dir, Source::AdlRoot));
        }
        warn!(
            "ADL_ROOT is set but {} does not contain sys/types.adl, ignoring it",
            dir.display()
        );
    }

    if let Some(dir) = locations
        .search_dirs
        .iter()
        .find_map(|dir| workspace_install(dir))
    {
        return Some((dir, Source::Workspace));
    }

    if let Some(adlc) = find_on_path(&locations.path, "adlc") {
        // Release archives are laid out as `bin/adlc` next to `lib/adl`.
        let beside_executable = adlc
            .canonicalize()
            .ok()
            .and_then(|exe| Some(exe.parent()?.parent()?.join("lib").join("adl")))
            .filter(|dir| is_stdlib_dir(dir));
        if let Some(dir) = beside_executable {
            return Some((dir, Source::CompilerOnPath));
        }

        if locations.query_compiler {
            let working_dir = locations.search_dirs.iter().find(|dir| dir.is_dir());
            if let Some(dir) = query_compiler(&adlc, working_dir.map(PathBuf::as_path)) {
                return Some((dir, Source::CompilerQuery));
            }
        }
    }

    locations
        .install_dirs
        .iter()
        .filter_map(|dir| newest_install(dir))
        .max_by(|(a, _), (b, _)| a.cmp(b))
        .map(|(_, dir)| (dir, Source::UserInstall))
}

/// Locate the standard library, falling back to the embedded copy, and make it the root that
/// [`is_root`] recognises. Returns `None` only if nothing was found and the embedded copy
/// could not be written.
pub fn activate(locations: &Locations) -> Option<PathBuf> {
    let located = locate(locations)
        .or_else(|| embedded_root().map(|root| (root.to_path_buf(), Source::Embedded)))
        .map(|(root, source)| (root.canonicalize().unwrap_or(root), source));

    match &located {
        Some((root, source)) => {
            info!(
                "using the ADL standard library at {} ({source})",
                root.display()
            )
        }
        None => warn!("no ADL standard library available; sys.* imports will not resolve"),
    }

    let root = located.map(|(root, _)| root);
    *ACTIVE_ROOT.write().expect("poisoned") = root.clone();
    root
}

/// Whether `path` is the package root of the standard library in use.
pub fn is_root(path: &Path) -> bool {
    ACTIVE_ROOT
        .read()
        .expect("poisoned")
        .as_deref()
        .is_some_and(|root| root == path)
}

/// The package root holding the embedded standard library, written out on first use.
pub fn embedded_root() -> Option<&'static Path> {
    static ROOT: OnceLock<Option<PathBuf>> = OnceLock::new();
    ROOT.get_or_init(|| {
        let dir = cache_dir().join(format!("stdlib-{}", env!("CARGO_PKG_VERSION")));
        materialize(&dir)
            .inspect_err(|e| {
                warn!(
                    "could not write the embedded ADL standard library to {}: {e}",
                    dir.display()
                )
            })
            .ok()
    })
    .as_deref()
}

fn env_dir(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn is_stdlib_dir(dir: &Path) -> bool {
    dir.join("sys").join("types.adl").is_file()
}

/// A toolchain installed by project tooling into `.local`, either in an ancestor of the search
/// dir or one directory below it (`<repo>/.local` and `<repo>/deno/.local` are both in use).
/// The search stops at the repository root.
fn workspace_install(search_dir: &Path) -> Option<PathBuf> {
    const MAX_ANCESTORS: usize = 8;
    let local_install = |dir: &Path| {
        let candidate = dir.join(".local").join("lib").join("adl");
        is_stdlib_dir(&candidate).then_some(candidate)
    };

    for ancestor in search_dir.ancestors().take(MAX_ANCESTORS) {
        if let Some(found) = local_install(ancestor) {
            return Some(found);
        }
        let mut children: Vec<PathBuf> = std::fs::read_dir(ancestor)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect();
        children.sort();
        if let Some(found) = children.iter().find_map(|child| local_install(child)) {
            return Some(found);
        }
        if ancestor.join(".git").exists() {
            break;
        }
    }
    None
}

fn find_on_path(path: &[PathBuf], name: &str) -> Option<PathBuf> {
    let names: &[String] = &if cfg!(windows) {
        vec![
            format!("{name}.exe"),
            format!("{name}.cmd"),
            name.to_string(),
        ]
    } else {
        vec![name.to_string()]
    };
    path.iter()
        .flat_map(|dir| names.iter().map(move |name| dir.join(name)))
        .find(|candidate| candidate.is_file())
}

/// Ask the compiler where its standard library is. Covers installs whose files are not beside
/// the executable on `PATH`: version manager shims, and cabal or nix builds.
fn query_compiler(adlc: &Path, working_dir: Option<&Path>) -> Option<PathBuf> {
    const TIMEOUT: Duration = Duration::from_secs(3);

    let mut command = Command::new(adlc);
    command
        .args(["show", "--adlstdlib"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // Version managers choose the toolchain from the directory the command runs in.
    if let Some(working_dir) = working_dir {
        command.current_dir(working_dir);
    }

    let mut child = command
        .spawn()
        .inspect_err(|e| debug!("could not run {}: {e}", adlc.display()))
        .ok()?;

    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < TIMEOUT => {
                std::thread::sleep(Duration::from_millis(20))
            }
            _ => {
                debug!(
                    "{} show --adlstdlib did not finish, giving up",
                    adlc.display()
                );
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };

    let mut output = String::new();
    child.stdout.take()?.read_to_string(&mut output).ok()?;
    let dir = PathBuf::from(output.trim());
    if status.success() && dir.is_absolute() && is_stdlib_dir(&dir) {
        Some(dir)
    } else {
        debug!(
            "{} show --adlstdlib did not report a standard library ({status})",
            adlc.display()
        );
        None
    }
}

/// The highest version installed under `dir`, laid out as `<dir>/<version>/lib/adl`.
fn newest_install(dir: &Path) -> Option<(Vec<u64>, PathBuf)> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let version = entry
                .file_name()
                .to_str()?
                .trim_start_matches('v')
                .split('.')
                .map(|part| part.parse::<u64>().ok())
                .collect::<Option<Vec<u64>>>()?;
            let stdlib = entry.path().join("lib").join("adl");
            is_stdlib_dir(&stdlib).then_some((version, stdlib))
        })
        .max_by(|(a, _), (b, _)| a.cmp(b))
}

/// Write the embedded standard library into `dir` and return the canonical package root.
///
/// Files whose contents already match are left untouched, so concurrent servers and repeated
/// startups do not rewrite files an editor may have open.
pub fn materialize(dir: &Path) -> std::io::Result<PathBuf> {
    let marker = ("adl-package.json", PACKAGE_MARKER);
    for (relative_path, contents) in FILES.iter().chain(std::iter::once(&marker)) {
        let path = dir.join(relative_path);
        if std::fs::read_to_string(&path).is_ok_and(|existing| existing == *contents) {
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, contents)?;
    }
    dir.canonicalize()
}

/// Tests share one throwaway directory rather than writing to the user's cache.
#[cfg(test)]
fn cache_dir() -> PathBuf {
    std::env::temp_dir().join(format!("adl-lsp-test-{}", std::process::id()))
}

/// A per-user cache directory, falling back to the system temp directory.
#[cfg(not(test))]
fn cache_dir() -> PathBuf {
    let base = env_dir("XDG_CACHE_HOME")
        .or_else(|| {
            if cfg!(target_os = "macos") {
                env_dir("HOME").map(|home| home.join("Library").join("Caches"))
            } else if cfg!(windows) {
                env_dir("LOCALAPPDATA")
            } else {
                env_dir("HOME").map(|home| home.join(".cache"))
            }
        })
        .unwrap_or_else(std::env::temp_dir);

    base.join("adl-lsp")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::AdlParser;
    use async_lsp::lsp_types::Url;

    #[test]
    fn test_materialize_writes_every_file() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let root = materialize(&temp_dir.path().join("stdlib")).unwrap();

        for (relative_path, contents) in FILES {
            let written = std::fs::read_to_string(root.join(relative_path)).unwrap();
            assert_eq!(&written, contents, "{relative_path}");
        }
        assert!(root.join("adl-package.json").exists());
        assert!(root.join("sys/types.adl").exists());
    }

    #[test]
    fn test_materialize_is_idempotent() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let dir = temp_dir.path().join("stdlib");
        let root = materialize(&dir).unwrap();

        let types = root.join("sys/types.adl");
        let modified_before = std::fs::metadata(&types).unwrap().modified().unwrap();
        assert_eq!(materialize(&dir).unwrap(), root);
        let modified_after = std::fs::metadata(&types).unwrap().modified().unwrap();
        assert_eq!(modified_before, modified_after);
    }

    #[test]
    fn test_materialize_repairs_modified_files() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let dir = temp_dir.path().join("stdlib");
        let root = materialize(&dir).unwrap();

        let types = root.join("sys/types.adl");
        std::fs::write(&types, "module sys.types {};").unwrap();
        materialize(&dir).unwrap();
        assert!(std::fs::read_to_string(&types).unwrap().contains("Pair"));
    }

    /// Lay out a toolchain install (`bin/adlc` next to `lib/adl`) under `prefix`.
    fn install_toolchain(prefix: &Path) -> PathBuf {
        let stdlib = prefix.join("lib").join("adl");
        std::fs::create_dir_all(stdlib.join("sys")).unwrap();
        std::fs::write(stdlib.join("sys/types.adl"), "module sys.types {};").unwrap();
        std::fs::create_dir_all(prefix.join("bin")).unwrap();
        std::fs::write(prefix.join("bin/adlc"), "").unwrap();
        stdlib
    }

    #[test]
    fn test_locate_finds_nothing_without_an_install() {
        let workspace = tempfile::TempDir::new().unwrap();
        let locations = Locations {
            search_dirs: vec![workspace.path().join("adl")],
            ..Locations::default()
        };
        assert_eq!(locate(&locations), None);
    }

    #[test]
    fn test_locate_configured_dir() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let stdlib = install_toolchain(temp_dir.path());
        let locations = Locations {
            configured: Some(stdlib.clone()),
            ..Locations::default()
        };
        assert_eq!(locate(&locations), Some((stdlib, Source::Configured)));
    }

    #[test]
    fn test_locate_skips_invalid_configured_dir() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let adl_root = temp_dir.path().join("adl-root");
        let stdlib = install_toolchain(&adl_root);
        let locations = Locations {
            configured: Some(temp_dir.path().join("missing")),
            adl_root: Some(adl_root),
            ..Locations::default()
        };
        assert_eq!(locate(&locations), Some((stdlib, Source::AdlRoot)));
    }

    #[test]
    fn test_locate_workspace_install() {
        // `<repo>/.local` and `<repo>/deno/.local` are both used by project tooling.
        for prefix in [".local", "deno/.local"] {
            let repo = tempfile::TempDir::new().unwrap();
            std::fs::create_dir_all(repo.path().join(".git")).unwrap();
            std::fs::create_dir_all(repo.path().join("adl")).unwrap();
            let stdlib = install_toolchain(&repo.path().join(prefix));
            let locations = Locations {
                search_dirs: vec![repo.path().join("adl")],
                ..Locations::default()
            };
            assert_eq!(
                locate(&locations),
                Some((stdlib, Source::Workspace)),
                "{prefix}"
            );
        }
    }

    #[test]
    fn test_locate_workspace_install_stops_at_repository_root() {
        let outside = tempfile::TempDir::new().unwrap();
        install_toolchain(&outside.path().join(".local"));
        let repo = outside.path().join("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::create_dir_all(repo.join("adl")).unwrap();
        let locations = Locations {
            search_dirs: vec![repo.join("adl")],
            ..Locations::default()
        };
        assert_eq!(locate(&locations), None);
    }

    #[test]
    fn test_locate_beside_compiler_on_path() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let prefix = temp_dir.path().canonicalize().unwrap().join("toolchain");
        let stdlib = install_toolchain(&prefix);
        let locations = Locations {
            path: vec![temp_dir.path().join("empty"), prefix.join("bin")],
            ..Locations::default()
        };
        assert_eq!(locate(&locations), Some((stdlib, Source::CompilerOnPath)));
    }

    #[cfg(unix)]
    #[test]
    fn test_locate_follows_symlinked_compiler() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let prefix = temp_dir.path().canonicalize().unwrap().join("toolchain");
        let stdlib = install_toolchain(&prefix);
        let bin = temp_dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::os::unix::fs::symlink(prefix.join("bin/adlc"), bin.join("adlc")).unwrap();
        let locations = Locations {
            path: vec![bin],
            ..Locations::default()
        };
        assert_eq!(locate(&locations), Some((stdlib, Source::CompilerOnPath)));
    }

    /// A shim on `PATH` has no files beside it, so the compiler is asked instead.
    #[cfg(unix)]
    #[test]
    fn test_locate_queries_compiler_shim() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = tempfile::TempDir::new().unwrap();
        let stdlib = install_toolchain(&temp_dir.path().join("elsewhere"));
        let shims = temp_dir.path().join("shims");
        std::fs::create_dir_all(&shims).unwrap();
        let shim = shims.join("adlc");
        std::fs::write(
            &shim,
            format!(
                "#!/bin/sh\n[ \"$1 $2\" = \"show --adlstdlib\" ] && echo '{}'\n",
                stdlib.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();

        let locations = Locations {
            path: vec![shims],
            query_compiler: true,
            ..Locations::default()
        };
        assert_eq!(locate(&locations), Some((stdlib, Source::CompilerQuery)));
    }

    #[cfg(unix)]
    #[test]
    fn test_locate_survives_failing_compiler() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = tempfile::TempDir::new().unwrap();
        let installs = temp_dir.path().join("installs");
        let stdlib = install_toolchain(&installs.join("1.2.0"));
        let shims = temp_dir.path().join("shims");
        std::fs::create_dir_all(&shims).unwrap();
        let shim = shims.join("adlc");
        std::fs::write(&shim, "#!/bin/sh\necho 'not configured' >&2\nexit 1\n").unwrap();
        std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();

        let locations = Locations {
            path: vec![shims],
            install_dirs: vec![installs],
            query_compiler: true,
            ..Locations::default()
        };
        assert_eq!(locate(&locations), Some((stdlib, Source::UserInstall)));
    }

    #[test]
    fn test_locate_prefers_newest_user_install() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let cache = temp_dir.path().join("cache");
        let proto = temp_dir.path().join("proto");
        install_toolchain(&cache.join("1.2.0"));
        install_toolchain(&cache.join("1.9.1"));
        let newest = install_toolchain(&proto.join("1.10.0"));
        // Not an install: the downloads directory of the cache wrapper script.
        std::fs::create_dir_all(cache.join("downloads")).unwrap();

        let locations = Locations {
            install_dirs: vec![cache, temp_dir.path().join("missing"), proto],
            ..Locations::default()
        };
        assert_eq!(locate(&locations), Some((newest, Source::UserInstall)));
    }

    /// Every embedded module must parse and declare the module name its path implies,
    /// otherwise imports of it cannot resolve.
    #[test]
    fn test_embedded_modules_match_their_paths() {
        let mut parser = AdlParser::new();
        for (relative_path, contents) in FILES {
            let Some(module_path) = relative_path.strip_suffix(".adl") else {
                continue;
            };
            let uri = Url::parse(&format!("file:///stdlib/{relative_path}")).unwrap();
            let tree = parser.parse(uri, contents).expect("stdlib module parses");
            assert_eq!(
                tree.find_module_name(contents.as_bytes()),
                Some(module_path.replace('/', ".").as_str()),
                "{relative_path}"
            );
        }
    }
}
