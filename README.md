# nix-path-pkgs

Ultra-fast CLI tool for displaying Nix packages in your Starship prompt.

## Description

Show which Nix packages you have in your PATH directly in your shell prompt. Designed for maximum speed (cold cache ~460ms, warm cache ~68ms) to avoid slowing down your prompt.

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
[custom.nix_packages]
command = "nix-path-pkgs"
when = "nix-path-pkgs"
format = "via [$symbol($output)]($style) "
symbol = "❄️ "
style = "bold blue"
```

### Configuration

**Environment Variables:**
- `NIX_PATH_PKGS_CACHE_TTL` - Cache TTL in seconds (default: 3600)
  - Set to `0` to disable caching
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

Benchmarked on Apple M2 (10 runs, median values):

|    Scenario    |  Time  | Description                       |
|----------------|--------|-----------------------------------|
| **Cold cache** | ~460ms | First run or after nixpkgs update |
| **Warm cache** | ~68ms  | Subsequent runs (cache hit)       |

### Breakdown

**Cold cache (~460ms):**
- Cache key generation (`nix eval`): ~90ms
- Stdenv requisites query (`nix eval`): ~370ms
- PATH parsing + filtering: <1ms

**Warm cache (~68ms):**
- Cache key generation (`nix eval`): ~68ms
- Read cached data: <1ms
- PATH parsing + filtering: <1ms

### Key Optimizations
1. **Single nix eval for cache key** - Combined flake revision + system detection
2. **Direct byte-level JSON parsing** - Skips serde_json deserialization for hash extraction
3. **Zero-copy string parsing** - Uses borrowed slices instead of allocations
4. **Pre-allocated collections** - HashSet/Vec with capacity hints
5. **Aggressive compiler flags** - LTO, single codegen unit, opt-level 3

## Testing

**32 comprehensive tests** covering functionality and performance:

```bash
cargo test --release              # All tests (32)
cargo test --test integration     # End-to-end tests (14)
cargo test --test unit            # Logic tests (18)
```

**Coverage:**
- Binary execution and output format
- Cache behavior (TTL=0, custom TTL, expiration)
- Edge cases (empty PATH, non-nix paths)
- Deduplication and skip list logic
- Performance regression (<5s cold, <500ms warm)
- Nix store path parsing and hash extraction

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

1. **Generate cache key** (always runs):
   ```bash
   nix eval --impure --raw --expr \
     '"${(builtins.getFlake "nixpkgs").rev}-${builtins.currentSystem}"'
   ```
   Output: `c12c63cd6c5eb34c7b4c3076c6a99e00fcab86ec-aarch64-darwin`

2. **Check cache**: Look for `~/.cache/nix-path-pkgs/{cache-key}-stdenv-allowed-requisites.json`

3. **Fetch stdenv packages** (only on cache miss):
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

**Empty output?**
- Check PATH has nix packages: `echo $PATH | grep nix/store`
- Try with cache disabled: `NIX_PATH_PKGS_CACHE_TTL=0 nix-path-pkgs`
- Check if packages are in skip list (see Configuration)

**Stale data?**
- Cache updates automatically when nixpkgs revision changes
- Manual refresh: `rm -rf ~/.cache/nix-path-pkgs && nix-path-pkgs`
- Old caches auto-cleanup after 24 hours

## License

This project is licensed under the MIT License. See the [LICENSE](LICENSE) file for details.
