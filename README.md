# depnuke

A fast CLI tool for nuking `node_modules`, lock files, and package manager caches during local npm package development.

If you've ever typed this:

```sh
rm -rf node_modules pnpm-lock.yaml && pnpm cache delete @sentry-internal/browser-utils @sentry/vue @sentry/browser
```

Now you can just type:

```sh
depnuke @sentry/vue
```

It detects your package manager, recursively walks the dependency tree, removes `node_modules` and lock files, and clears every dependency from the cache.

## Install

```sh
cargo install depnuke
```

Or build from source:

```sh
git clone https://github.com/logaretm/depnuke.git
cd depnuke
cargo install --path .
```

## Usage

```sh
# Nuke everything: remove node_modules, lock file, and clear cache for a package + all its deps
depnuke @sentry/vue

# Multiple packages
depnuke @sentry/vue @sentry/browser

# Only clear caches, keep node_modules and lock file
depnuke --cache-only @sentry/vue

# Limit dependency depth (0 = package itself only, 1 = direct deps, etc.)
depnuke -d 0 @sentry/vue     # just the package
depnuke -d 1 @sentry/vue     # package + its direct deps

# Include devDependencies
depnuke --dev @sentry/vue

# Include optionalDependencies
depnuke --optional @sentry/vue

# Include everything (dev + optional + peer)
depnuke --all @sentry/vue

# Just clean node_modules and lock file, no cache clearing
depnuke
```

## How it works

1. **Detects your package manager** from lock files: `pnpm-lock.yaml` / `yarn.lock` / `package-lock.json`
2. **Collects dependencies** by reading `package.json` files from `node_modules` using BFS traversal
3. **Removes `node_modules`** and the lock file (in a background thread)
4. **Clears the cache** for every collected dependency using the detected manager's native cache commands (up to 8 concurrent processes)

### Package manager support

| Manager | Detection | Cache command |
|---------|-----------|---------------|
| pnpm | `pnpm-lock.yaml` or `pnpm-workspace.yaml` | `pnpm cache delete <pkg>` |
| yarn | `yarn.lock` | `yarn cache clean <pkg>` |
| npm | `package-lock.json` (fallback) | `npm cache clean <pkg> --force` |

### node_modules layouts

Handles all common `node_modules` structures:

- **Flat** (`npm`, `yarn classic`) — `node_modules/<pkg>/package.json`
- **Symlinked** (`pnpm`) — follows symlinks from the hoisted structure
- **pnpm store** — `node_modules/.pnpm/<pkg>@<version>/node_modules/<pkg>/package.json`
- **Nested** (`npm --legacy-bundling`, `yarn workspaces`) — recursive walk up to 5 levels deep

### Scoped package detection

When you pass a scoped package like `@sentry/vue`, depnuke also finds all other packages under the same scope (`@sentry/*`) from your root `package.json` and includes them automatically.

## Options

```
Arguments:
  [PACKAGES]...          Package names to nuke from cache (reads their deps too)

Options:
      --cache-only       Only clean caches, skip removing node_modules and lock files
  -d, --depth <DEPTH>    Dependency depth (0 = self only, 1 = direct deps, etc.)
      --dev              Also include devDependencies
      --optional         Also include optionalDependencies
      --all              Include all dependency types (dev + optional + peer)
  -h, --help             Print help
```

## License

MIT
