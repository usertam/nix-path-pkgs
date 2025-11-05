# nix-path-pkgs

Ultra-fast CLI tool for displaying Nix packages in your Starship prompt.

## Description

Show which Nix packages you have in your PATH directly in your shell prompt. Designed for maximum speed (cold cache ~410ms, warm cache **~2ms**) to avoid slowing down your prompt.

**Requirements:**
- This tool is designed for use with **Nix Flakes**
- You must pin your system's nixpkgs in `/etc/nix/registry.json` or have a `flake.lock` in your current directory
- The tool reads nixpkgs revision from these sources to generate cache keys

**Smart filtering:**
- When using a project with `flake.lock`, filters out stdenv packages from **both** project and system nixpkgs
- Ensures clean output even when you have packages from multiple nixpkgs versions in PATH

Filters out nixpkgs stdenv packages to show only your custom additions:

1. Queries nixpkgs stdenv allowed requisites (with intelligent caching)
2. Parses your PATH for nix store entries
3. Filters out standard packages and duplicates
4. Outputs a clean, comma-separated list

Perfect for Starship's `custom` module to show active Nix packages without performance impact.

## Installation

```bash
cargo build --release
cp target/release/nix-path-pkgs ~/.local/bin/  # or anywhere in your PATH
```

### System Setup (Required)

This tool requires nixpkgs to be pinned in `/etc/nix/registry.json`. This is automatically done by NixOS and nix-darwin when using flakes.

**For standalone Nix installations**, add this to your `/etc/nix/nix.conf`:
```
experimental-features = nix-command flakes
```

Then pin nixpkgs:
```bash
# Pin to nixpkgs-unstable (or your preferred channel)
nix registry pin nixpkgs

# Verify the pin
cat /etc/nix/registry.json
```

**Alternative**: If you're anywhere inside a directory tree containing `flake.lock`, the tool will automatically find it (traversing upward like git) and use the nixpkgs revision for project-specific caching.

## Usage

### Basic Usage

```bash
# Basic usage (with default 1-hour cache)
nix-path-pkgs

# Disable cache (always fresh data)
NIX_PATH_PKGS_CACHE_TTL=0 nix-path-pkgs

# Custom cache TTL (in seconds)
NIX_PATH_PKGS_CACHE_TTL=7200 nix-path-pkgs
```

### Example Output

```
cargo, gh, ripgrep, fd, bat
```

### Starship Integration

Add to your `~/.config/starship.toml`:

```toml
[custom.nix_path_pkgs]
command = "nix-path-pkgs"
when = "nix-path-pkgs"
format = "via [$symbol($output)]($style) "
symbol = "❄️ "
style = "bold blue"
ignore_timeout = true   # Optional
```

The warm cache is fast enough (~2ms) that you don't need `ignore_timeout`.

**Note:** Cold cache runs (~410ms median, up to ~800ms on first run) may occasionally exceed the default starship `command_timeout` of 500ms. Consider:
- Setting `command_timeout = 1000` in your starship config, OR
- Using `ignore_timeout = true` (recommended - the tool handles its own timeout via `NIX_PATH_PKGS_TIMEOUT`)

### Configuration

**Environment Variables:**
- `NIX_PATH_PKGS_CACHE_TTL` - Cache TTL in seconds (default: 3600)
  - Set to `0` to disable caching
- `NIX_PATH_PKGS_TIMEOUT` - Nix command timeout in seconds (default: 2)
  - Set to `0` to disable timeout (fastest, but may hang if nix is unresponsive)
  - Increase if nix commands timeout on slow systems
- `XDG_CACHE_HOME` - Cache directory (default: `~/.cache`)

**Cache Location:**
- `$XDG_CACHE_HOME/nix-path-pkgs/` or `~/.cache/nix-path-pkgs/`
- Cache files are named: `{nixpkgs-rev}-{system}-stdenv-allowed-requisites.json`

**Skip List:**
Certain packages are always excluded (edit `SKIP` constant in `src/main.rs` to customize):
- `bash-interactive` - Nix's interactive bash
- `ghostty` - Terminal emulator
- `ghostty-bin` - Terminal emulator binary

## Performance

Benchmarked on Apple M2 (median of 10/20 runs):

|    Scenario    |  Median  |  Range   | Description                       |
|----------------|----------|----------|-----------------------------------|
| **Cold cache** | **410ms** | 405-791ms | First run or after nixpkgs update |
| **Warm cache** | **2ms**   | 2-2ms    | Subsequent runs (cache hit)       |
| **Direct nix** | 405ms    | 402-421ms | Baseline (`nix eval` performance) |

**Note:** First cold run may be ~800ms due to nix daemon startup, subsequent cold runs stabilize at ~410ms.

### Breakdown

**Cold cache (~410ms median):**
- Cache key generation (read `/etc/nix/registry.json` or `flake.lock`): <1ms
- Stdenv requisites query (`nix eval`): ~405ms
- Tool overhead: ~5ms
- If project and system nixpkgs differ: queries both **in parallel** (~810ms total, not 1620ms!)
- PATH parsing + filtering: <1ms

**Warm cache (~2ms):**
- Cache key generation (read `/etc/nix/registry.json`): <1ms
- Read cached data: <1ms
- PATH parsing + filtering: <1ms

### Key Optimizations
1. **Read `flake.lock` or `/etc/nix/registry.json` directly** - No nix calls for cache key (saves ~68ms per run!)
   - Project-specific caching via `flake.lock` (searches upward like git)
   - System-wide caching via `/etc/nix/registry.json`
2. **Parallel nix queries** - When both project and system need querying, runs them concurrently (~810ms not 1620ms!)
3. **Independent cache management** - Project and system caches are separate, maximizes reuse
4. **Direct nix binary path** - Avoids PATH search (eliminates 24 failed syscalls)
5. **Cached binary path lookup** - Uses `OnceLock` to avoid repeated stat calls
6. **Optional timeout mechanism** - Disable thread spawning overhead with `NIX_PATH_PKGS_TIMEOUT=0`
7. **Direct byte-level JSON parsing** - Skips serde_json deserialization for hash extraction
8. **Zero-copy string parsing** - Uses borrowed slices instead of allocations
9. **Pre-allocated collections** - HashSet/Vec with capacity hints
10. **Aggressive compiler flags** - LTO, single codegen unit, opt-level 3

## Testing

**44 comprehensive tests** covering functionality and performance:

```bash
cargo test --release              # All tests (44)
cargo test --test integration     # End-to-end tests (14)
cargo test --test unit            # Logic tests (20)
cargo test --test scenarios       # Cache scenarios (10)
```

**Coverage:**
- Binary execution and output format
- Cache behavior (TTL=0, custom TTL, expiration)
- **All 6 cache scenarios** (project/system combinations)
- Independent cache management (project vs system)
- Parallel query optimization
- Edge cases (empty PATH, non-nix paths)
- Deduplication and skip list logic
- Performance regression tests
- Nix store path parsing and hash extraction
- flake.lock upward traversal

## Build Optimizations

The `Cargo.toml` includes aggressive release profile:

```toml
[profile.release]
lto = true              # Link-time optimization
codegen-units = 1       # Better optimization
opt-level = 3           # Maximum optimization
strip = true            # Smaller binary
panic = "abort"         # Faster panic
```

Binary size: ~395KB (stripped)

## How It Works

1. **Generate cache key** (always runs, <1ms):
   - **First**: Searches for `flake.lock` by traversing upward from CWD (like git)
     - Checks current directory, then parent, then grandparent, etc.
     - Enables project-specific caching anywhere in your project tree
   - **Then**: Falls back to `/etc/nix/registry.json` (system-wide)
   - **Finally**: Falls back to `nix eval` if neither available
   - Detects current system (e.g., `aarch64-darwin`)
   - Output: `a7fc11be66bdfb5cdde611ee5ce381c183da8386-aarch64-darwin`

2. **Check cache**: Look for `~/.cache/nix-path-pkgs/{cache-key}-stdenv-allowed-requisites.json`

3. **Fetch stdenv packages** (only on cache miss):
   - If both project and system nixpkgs detected with different revisions:
     - Queries stdenv from **both** nixpkgs versions **in parallel**
     - Runs both `nix eval` commands concurrently (saves ~810ms!)
     - Merges the results to filter packages from both
   - Otherwise queries from single nixpkgs source
   ```nix
   with builtins.getFlake "nixpkgs";
   with legacyPackages.${builtins.currentSystem};
   lib.filter lib.isDerivation stdenv.allowedRequisites
   ```

4. **Parse $PATH**: Extract package names from nix store paths
   - Path format: `/nix/store/{32-char-hash}-{name}-{version}/bin`
   - Strips version numbers (e.g., `bash-5.2-p15` → `bash`)

5. **Filter and deduplicate**:
   - Remove stdenv packages (bash, coreutils, etc.)
   - Remove skip list packages
   - Remove duplicates (keep first occurrence)
   - Output remaining packages

6. **Output**: Comma-separated list to stdout

## Exit Codes

| Code | Meaning                               |
|------|---------------------------------------|
| `0`  | Success - non-standard packages found |
| `1`  | No non-standard packages in PATH      |

## Troubleshooting

**Slow performance?**
- Check cache exists: `ls ~/.cache/nix-path-pkgs/`
- Verify cache TTL: `echo $NIX_PATH_PKGS_CACHE_TTL`
- Cold cache is normal after nixpkgs updates
- Ensure `/etc/nix/registry.json` exists or you have a `flake.lock` in CWD

**Empty output?**
- Check PATH has nix packages: `echo $PATH | grep nix/store`
- Try with cache disabled: `NIX_PATH_PKGS_CACHE_TTL=0 nix-path-pkgs`
- Check if packages are in skip list (see Configuration)

**Stale data?**
- Cache updates automatically when nixpkgs revision changes
- Manual refresh: `rm -rf ~/.cache/nix-path-pkgs && nix-path-pkgs`
- Old caches auto-cleanup after 24 hours

**"No cache key" or falling back to nix eval?**
- Ensure flakes are enabled: `nix --version` (should show flakes support)
- Pin nixpkgs: `nix registry pin nixpkgs`
- Check `/etc/nix/registry.json` exists and contains nixpkgs
- Or run from anywhere inside a project with `flake.lock` (searches upward automatically)

## License

This project is licensed under the MIT License. See the [LICENSE](LICENSE) file for details.
