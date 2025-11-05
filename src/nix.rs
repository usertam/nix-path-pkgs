use std::{
    collections::HashSet,
    env,
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::{mpsc, OnceLock},
    thread,
    time::Duration,
};

const NIX_EXPR: &str = r#"
with builtins.getFlake "nixpkgs";
with legacyPackages.${builtins.currentSystem};
lib.filter lib.isDerivation stdenv.allowedRequisites
"#;

// Get timeout duration from env var, or use default
pub fn get_timeout() -> Option<Duration> {
    match env::var("NIX_PATH_PKGS_TIMEOUT") {
        Ok(s) => match s.parse::<u64>() {
            Ok(0) => None, // 0 = no timeout (fastest, but may hang)
            Ok(secs) => Some(Duration::from_secs(secs)),
            Err(_) => Some(Duration::from_secs(2)), // Invalid, use default
        },
        Err(_) => Some(Duration::from_secs(2)), // Not set, use default
    }
}

// Run a command with optional timeout. Returns None if timeout occurs or command fails.
pub fn run_command(mut cmd: Command, timeout: Option<Duration>) -> Option<Output> {
    match timeout {
        Some(duration) => {
            // Use timeout mechanism (spawns thread)
            let (tx, rx) = mpsc::channel();
            thread::spawn(move || {
                let result = cmd.output();
                let _ = tx.send(result);
            });
            match rx.recv_timeout(duration) {
                Ok(Ok(output)) => Some(output),
                _ => None, // Timeout or command failed
            }
        }
        None => {
            // No timeout - faster but may hang
            cmd.output().ok()
        }
    }
}

// Find nix binary path (avoid PATH search overhead - 24 failed posix_spawn calls!)
// Cached to avoid repeated stat syscalls
pub fn find_nix() -> &'static str {
    static NIX_PATH: OnceLock<&'static str> = OnceLock::new();

    NIX_PATH.get_or_init(|| {
        // Common nix locations in order of likelihood
        const NIX_PATHS: &[&str] = &[
            "/nix/var/nix/profiles/default/bin/nix",    // Default profile
            "/run/current-system/sw/bin/nix",           // NixOS/nix-darwin
        ];

        for path in NIX_PATHS {
            if std::path::Path::new(path).exists() {
                return *path;
            }
        }

        "nix" // Fallback to PATH search
    })
}

// Find flake.lock by traversing upward from current directory
// Similar to how git finds .git directory
fn find_flake_lock() -> Option<PathBuf> {
    let mut current = env::current_dir().ok()?;

    loop {
        let flake_lock = current.join("flake.lock");
        if flake_lock.exists() {
            return Some(flake_lock);
        }

        // Try parent directory
        if !current.pop() {
            // Reached root, no flake.lock found
            return None;
        }
    }
}

// Extract nixpkgs revision from flake.lock (searches upward from CWD)
// Format: {"nodes":{"nixpkgs":{"locked":{"rev":"abc123..."}}}}
fn get_nixpkgs_rev_from_flake_lock() -> Option<String> {
    let flake_lock_path = find_flake_lock()?;
    let json = fs::read_to_string(flake_lock_path).ok()?;

    // Look for: "nodes" → "nixpkgs" → "locked" → "rev"
    // Find the nixpkgs node
    let nodes_pos = json.find(r#""nodes""#)?;
    let nixpkgs_pos = json[nodes_pos..].find(r#""nixpkgs""#)?;
    let search_start = nodes_pos + nixpkgs_pos;

    // Find "locked" object after nixpkgs
    let locked_pos = json[search_start..].find(r#""locked""#)?;
    let locked_start = search_start + locked_pos;

    // Find "rev" field in locked object
    let rev_pos = json[locked_start..].find(r#""rev":""#)?;
    let rev_start = locked_start + rev_pos + 7; // Skip past "rev":"

    // Find the closing quote
    let rev_end = json[rev_start..].find('"')?;

    Some(json[rev_start..rev_start + rev_end].to_string())
}

// Extract nixpkgs revision from /etc/nix/registry.json
// Format: {"flakes":[{"from":{"id":"nixpkgs",...},"to":{"rev":"abc123..."}}]}
fn get_nixpkgs_rev_from_registry() -> Option<String> {
    let json = fs::read_to_string("/etc/nix/registry.json").ok()?;

    // Fast path: find "nixpkgs" in "from" field, then extract the "rev" from "to"
    // Look for pattern: "id":"nixpkgs" ... "rev":"<hash>"
    let nixpkgs_pos = json.find(r#""id":"nixpkgs""#)?;

    // Find "rev" field after the nixpkgs entry (within the same flake entry)
    let search_start = nixpkgs_pos + 14; // Skip past "id":"nixpkgs"
    let rev_pos = json[search_start..].find(r#""rev":""#)?;
    let rev_start = search_start + rev_pos + 7; // Skip past "rev":"

    // Find the closing quote
    let rev_end = json[rev_start..].find('"')?;

    Some(json[rev_start..rev_start + rev_end].to_string())
}

// Get nixpkgs revisions from all available sources
// Returns (project_rev, system_rev) - either or both may be Some
pub fn get_nixpkgs_revs() -> (Option<String>, Option<String>) {
    let project_rev = get_nixpkgs_rev_from_flake_lock();
    let system_rev = get_nixpkgs_rev_from_registry();
    (project_rev, system_rev)
}

// Get current system string (e.g., "aarch64-darwin", "x86_64-linux")
fn get_system() -> String {
    let arch = match env::consts::ARCH {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        "arm" => "armv7l",
        "x86" => "i686",
        other => other,
    };
    let os = match env::consts::OS {
        "macos" => "darwin",
        "linux" => "linux",
        other => other,
    };
    format!("{}-{}", arch, os)
}

// Create cache key from revision
pub fn make_cache_key(rev: &str) -> String {
    format!("{}-{}", rev, get_system())
}

// Query stdenv packages from a specific nixpkgs revision
pub fn query_stdenv(rev: &str) -> Vec<u8> {
    let expr = format!(
        r#"with builtins.getFlake "github:nixos/nixpkgs/{}";
with legacyPackages.${{builtins.currentSystem}};
lib.filter lib.isDerivation stdenv.allowedRequisites"#,
        rev
    );

    let mut cmd = Command::new(find_nix());
    cmd.args(["eval", "--impure", "--json", "--expr", &expr]);

    match run_command(cmd, get_timeout()) {
        Some(output) if output.status.success() => output.stdout,
        _ => Vec::new(),
    }
}

// Query stdenv from both project and system (for no-cache mode)
pub fn query_stdenv_multi(
    project_rev: &Option<String>,
    system_rev: &Option<String>,
) -> Vec<u8> {
    match (project_rev, system_rev) {
        (Some(proj_rev), Some(sys_rev)) if proj_rev != sys_rev => {
            // Different revisions - query both in parallel!
            use std::thread;

            let proj_rev_clone = proj_rev.clone();
            let sys_rev_clone = sys_rev.clone();

            let proj_handle = thread::spawn(move || query_stdenv(&proj_rev_clone));
            let sys_handle = thread::spawn(move || query_stdenv(&sys_rev_clone));

            // Wait for both (~400ms instead of ~800ms!)
            let project_stdenv = proj_handle.join().unwrap_or_default();
            let system_stdenv = sys_handle.join().unwrap_or_default();

            merge_stdenv_json(&project_stdenv, &system_stdenv)
        }
        (_, Some(sys_rev)) => query_stdenv(sys_rev),
        (Some(proj_rev), None) => query_stdenv(proj_rev),
        (None, None) => query_stdenv_fallback(),
    }
}

// Fallback query when no rev found
pub fn query_stdenv_fallback() -> Vec<u8> {
    let mut cmd = Command::new(find_nix());
    cmd.args(["eval", "--impure", "--json", "--expr", NIX_EXPR]);

    match run_command(cmd, get_timeout()) {
        Some(output) if output.status.success() => output.stdout,
        _ => Vec::new(),
    }
}

// Merge two JSON arrays of stdenv packages
pub fn merge_stdenv_json(json1: &[u8], json2: &[u8]) -> Vec<u8> {
    let text1 = std::str::from_utf8(json1).unwrap_or("[]");
    let text2 = std::str::from_utf8(json2).unwrap_or("[]");

    // Extract all paths from both arrays
    let mut paths = HashSet::new();

    for text in [text1, text2] {
        let mut i = 0;
        let bytes = text.as_bytes();
        while i < bytes.len() {
            if bytes.get(i..i + 11) == Some(b"/nix/store/") {
                // Find the closing quote
                if let Some(end) = text[i..].find('"') {
                    if text.is_char_boundary(i) && text.is_char_boundary(i + end) {
                        paths.insert(text[i..i + end].to_string());
                    }
                }
            }
            i += 1;
        }
    }

    // Convert back to JSON array
    let mut result = String::from("[");
    for (idx, path) in paths.iter().enumerate() {
        if idx > 0 {
            result.push(',');
        }
        result.push('"');
        result.push_str(path);
        result.push('"');
    }
    result.push(']');

    result.into_bytes()
}
