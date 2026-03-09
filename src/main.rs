use clap::Parser;
use serde::de::IgnoredAny;
use serde::Deserialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;

#[derive(Parser)]
#[command(name = "nuke-pkg", about = "Nuke node_modules, lock files, and package manager caches")]
struct Cli {
    /// Package names to nuke from cache (reads their deps too)
    packages: Vec<String>,

    /// Only clean caches, skip removing node_modules and lock files
    #[arg(long)]
    cache_only: bool,

    /// Dependency depth to traverse (0 = package itself only, 1 = direct deps, etc.)
    /// Omit for unlimited depth
    #[arg(short, long)]
    depth: Option<u32>,

    /// Also include devDependencies
    #[arg(long)]
    dev: bool,

    /// Also include optionalDependencies
    #[arg(long)]
    optional: bool,

    /// Include all dependency types (dev + optional + peer)
    #[arg(long)]
    all: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum PackageManager {
    Pnpm,
    Yarn,
    Npm,
}

impl PackageManager {
    fn lock_file(&self) -> &str {
        match self {
            Self::Pnpm => "pnpm-lock.yaml",
            Self::Yarn => "yarn.lock",
            Self::Npm => "package-lock.json",
        }
    }

    fn name(&self) -> &str {
        match self {
            Self::Pnpm => "pnpm",
            Self::Yarn => "yarn",
            Self::Npm => "npm",
        }
    }

    fn cache_delete_args(&self, pkg: &str) -> Vec<String> {
        match self {
            Self::Pnpm => vec!["cache".into(), "delete".into(), pkg.into()],
            Self::Yarn => vec!["cache".into(), "clean".into(), pkg.into()],
            Self::Npm => vec!["cache".into(), "clean".into(), pkg.into(), "--force".into()],
        }
    }
}

// Only deserialize keys, ignore version values entirely
#[derive(Deserialize)]
struct PackageJson {
    #[serde(default)]
    dependencies: HashMap<String, IgnoredAny>,
    #[serde(default, rename = "devDependencies")]
    dev_dependencies: HashMap<String, IgnoredAny>,
    #[serde(default, rename = "peerDependencies")]
    peer_dependencies: HashMap<String, IgnoredAny>,
    #[serde(default, rename = "optionalDependencies")]
    optional_dependencies: HashMap<String, IgnoredAny>,
}

#[derive(Clone, Copy)]
struct DepFilter {
    dev: bool,
    optional: bool,
}

fn detect_package_manager() -> PackageManager {
    if Path::new("pnpm-lock.yaml").exists() || Path::new("pnpm-workspace.yaml").exists() {
        PackageManager::Pnpm
    } else if Path::new("yarn.lock").exists() {
        PackageManager::Yarn
    } else {
        PackageManager::Npm
    }
}

fn parse_pkg_json(pkg_path: &Path) -> Option<PackageJson> {
    let content = fs::read_to_string(pkg_path).ok()?;
    serde_json::from_str(&content).ok()
}

fn extract_dep_names(pkg: &PackageJson, filter: DepFilter) -> Vec<String> {
    let mut names = Vec::new();
    let always: [&HashMap<String, IgnoredAny>; 2] = [&pkg.dependencies, &pkg.peer_dependencies];
    for map in always {
        names.extend(map.keys().cloned());
    }
    if filter.dev {
        names.extend(pkg.dev_dependencies.keys().cloned());
    }
    if filter.optional {
        names.extend(pkg.optional_dependencies.keys().cloned());
    }
    names
}

/// Pre-built index of .pnpm store: maps "pkgname" -> path to its versioned dir
fn build_pnpm_index() -> HashMap<String, PathBuf> {
    let pnpm_dir = Path::new("node_modules/.pnpm");
    let mut index: HashMap<String, PathBuf> = HashMap::new();
    let Ok(entries) = fs::read_dir(pnpm_dir) else {
        return index;
    };
    for entry in entries.flatten() {
        let dir_name = entry.file_name();
        let dir_name = dir_name.to_string_lossy();
        // Format: @scope+name@version or name@version
        // Find the last @ that separates name from version
        if let Some(at_pos) = dir_name.rfind('@').filter(|&p| p > 0) {
            let pnpm_name = &dir_name[..at_pos];
            // Convert back: @scope+name -> @scope/name
            let pkg_name = pnpm_name.replacen('+', "/", 1);
            index.entry(pkg_name).or_insert_with(|| entry.path());
        }
    }
    index
}

/// Find a package's package.json across all possible node_modules layouts
fn find_pkg_json(name: &str, pnpm_index: &HashMap<String, PathBuf>) -> Option<PackageJson> {
    let nm = Path::new("node_modules");

    // 1. Standard flat path (also follows symlinks, covers pnpm hoisted links)
    let flat = nm.join(name).join("package.json");
    if let Some(pkg) = parse_pkg_json(&flat) {
        return Some(pkg);
    }

    // 2. pnpm .pnpm store — O(1) lookup from pre-built index
    if let Some(store_path) = pnpm_index.get(name) {
        let candidate = store_path.join("node_modules").join(name).join("package.json");
        if let Some(pkg) = parse_pkg_json(&candidate) {
            return Some(pkg);
        }
    }

    // 3. Nested node_modules (npm legacy-bundling, yarn workspaces)
    fn walk_nested(name: &str, dir: &Path, depth: u8) -> Option<PackageJson> {
        if depth > 5 {
            return None;
        }
        let candidate = dir.join("node_modules").join(name).join("package.json");
        if let Some(pkg) = parse_pkg_json(&candidate) {
            return Some(pkg);
        }
        let nm = dir.join("node_modules");
        if !nm.is_dir() {
            return None;
        }
        let Ok(entries) = fs::read_dir(&nm) else {
            return None;
        };
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                if let Some(pkg) = walk_nested(name, &entry.path(), depth + 1) {
                    return Some(pkg);
                }
            }
        }
        None
    }

    walk_nested(name, Path::new("."), 0)
}

fn collect_deps(
    packages: &[String],
    max_depth: Option<u32>,
    filter: DepFilter,
    pnpm_index: &HashMap<String, PathBuf>,
) -> Vec<String> {
    let mut visited: HashSet<String> = HashSet::new();
    let mut queue: VecDeque<(String, u32)> = VecDeque::new();

    for p in packages {
        queue.push_back((p.clone(), 0));
    }

    // Pull in scoped siblings from root package.json (depth 0)
    if let Some(root_pkg) = parse_pkg_json(Path::new("package.json")) {
        let root_deps = extract_dep_names(&root_pkg, filter);
        for dep in root_deps {
            for pkg_name in packages {
                if let Some(scope) = pkg_name.split('/').next() {
                    if scope.starts_with('@') && dep.starts_with(scope) {
                        queue.push_back((dep, 0));
                        break;
                    }
                }
            }
        }
    }

    // BFS with depth tracking
    while let Some((name, depth)) = queue.pop_front() {
        if !visited.insert(name.clone()) {
            continue;
        }

        if max_depth.is_some_and(|max| depth >= max) {
            continue;
        }

        match find_pkg_json(&name, pnpm_index) {
            Some(pkg) => {
                for dep in extract_dep_names(&pkg, filter) {
                    if !visited.contains(&dep) {
                        queue.push_back((dep, depth + 1));
                    }
                }
            }
            None => {
                eprintln!("  note: {} not found in node_modules", name);
            }
        }
    }

    let mut result: Vec<String> = visited.into_iter().collect();
    result.sort();
    result
}

fn remove_path(path: &str) {
    let p = Path::new(path);
    if p.exists() {
        if p.is_dir() {
            match fs::remove_dir_all(p) {
                Ok(()) => eprintln!("  removed {path}"),
                Err(e) => eprintln!("  failed to remove {path}: {e}"),
            }
        } else {
            match fs::remove_file(p) {
                Ok(()) => eprintln!("  removed {path}"),
                Err(e) => eprintln!("  failed to remove {path}: {e}"),
            }
        }
    }
}

const MAX_CONCURRENT: usize = 8;

fn clear_cache(pm: PackageManager, packages: &[String]) {
    let mut children = Vec::with_capacity(MAX_CONCURRENT);

    for pkg in packages {
        let args = pm.cache_delete_args(pkg);
        eprintln!("  {} {}", pm.name(), args.join(" "));

        match Command::new(pm.name())
            .args(&args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => children.push((pkg.clone(), child)),
            Err(e) => eprintln!("  error spawning {}: {e}", pm.name()),
        }

        // Drain when we hit the concurrency limit
        if children.len() >= MAX_CONCURRENT {
            for (name, mut child) in children.drain(..) {
                match child.wait() {
                    Ok(s) if s.success() => {}
                    Ok(s) => eprintln!("  warning: {} failed for {}: {}", pm.name(), name, s),
                    Err(e) => eprintln!("  error waiting for {}: {e}", name),
                }
            }
        }
    }

    // Wait for remaining
    for (name, mut child) in children {
        match child.wait() {
            Ok(s) if s.success() => {}
            Ok(s) => eprintln!("  warning: {} failed for {}: {}", pm.name(), name, s),
            Err(e) => eprintln!("  error waiting for {}: {e}", name),
        }
    }
}

fn main() {
    let cli = Cli::parse();
    let pm = detect_package_manager();

    eprintln!("detected package manager: {}", pm.name());

    let filter = DepFilter {
        dev: cli.dev || cli.all,
        optional: cli.optional || cli.all,
    };

    // Build pnpm index once before anything else
    let pnpm_index = build_pnpm_index();

    // Collect deps BEFORE removing node_modules (we need to read them)
    let deps = if !cli.packages.is_empty() {
        collect_deps(&cli.packages, cli.depth, filter, &pnpm_index)
    } else {
        vec![]
    };

    // Drop the index, we're done with it
    drop(pnpm_index);

    if !cli.cache_only {
        eprintln!("\ncleaning local artifacts...");

        let lock_file = pm.lock_file().to_string();

        // Remove node_modules in background thread while we proceed to cache clearing
        let nm_handle = if Path::new("node_modules").exists() {
            Some(thread::spawn(|| {
                match fs::remove_dir_all("node_modules") {
                    Ok(()) => eprintln!("  removed node_modules"),
                    Err(e) => eprintln!("  failed to remove node_modules: {e}"),
                }
            }))
        } else {
            None
        };

        // Lock file removal is fast, do it inline
        remove_path(&lock_file);

        if !deps.is_empty() {
            eprintln!("\nnuking {} package(s) from {} cache:", deps.len(), pm.name());
            clear_cache(pm, &deps);
        }

        // Wait for node_modules removal to finish
        if let Some(handle) = nm_handle {
            let _ = handle.join();
        }
    } else if !deps.is_empty() {
        eprintln!("\nnuking {} package(s) from {} cache:", deps.len(), pm.name());
        clear_cache(pm, &deps);
    }

    if !deps.is_empty() {
        eprintln!("\ndone.");
    }
}
