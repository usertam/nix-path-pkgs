use std::{
    collections::HashSet, env, fs, io,
    path::{Path, PathBuf},
    process::{Command, ExitCode, Output},
    sync::mpsc,
    thread,
    time::{Duration, SystemTime},
};

const NIX_EXPR: &str = r#"
with builtins.getFlake "nixpkgs";
with legacyPackages.${builtins.currentSystem};
lib.filter lib.isDerivation stdenv.allowedRequisites
"#;

const SKIP: &[&str] = &["bash-interactive", "ghostty", "ghostty-bin"];

// Run a command with a timeout. Returns None if timeout occurs.
pub fn run_with_timeout(mut cmd: Command, timeout: Duration) -> Option<Output> {
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        let result = cmd.output();
        let _ = tx.send(result);
    });

    match rx.recv_timeout(timeout) {
        Ok(Ok(output)) => Some(output),
        _ => None, // Timeout or command failed
    }
}

fn main() -> ExitCode {
    // cache TTL (secs). TTL=0 => no cache (no read, no write).
    let ttl: u64 = env::var("NIX_PATH_PKGS_CACHE_TTL")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3600);

    // Get cache metadata once (avoid redundant nix calls)
    let cache_key = if ttl > 0 {
        get_cache_key()
    } else {
        None
    };

    // nix eval output (cached unless TTL=0)
    let bytes = if ttl == 0 {
        refresh(false, None)
    } else {
        read_cache(ttl, cache_key.as_deref())
            .ok()
            .flatten()
            .unwrap_or_else(|| refresh(true, cache_key.as_deref()))
    };
    let ignore = parse_hashes(&bytes);

    // Walk $PATH in order; keep first occurrence only.
    let mut ordered: Vec<&str> = Vec::with_capacity(32);
    let mut seen: HashSet<&str> = HashSet::with_capacity(32);

    let path = env::var("PATH").unwrap_or_default();
    for dir in path.split(':').filter(|s| !s.is_empty()) {
        if let Some((h, name)) = hash_and_name(dir) {
            if ignore.contains(h) || SKIP.contains(&name) || name.is_empty() {
                continue;
            }
            if seen.insert(name) {
                ordered.push(name);
            }
        }
    }

    if !ordered.is_empty() {
        println!("{}", ordered.join(", "));
        ExitCode::from(0)
    } else {
        ExitCode::from(1)
    }
}

fn get_cache_key() -> Option<String> {
    // Get revision-system key in one nix call (no JSON parsing needed)
    let mut cmd = Command::new("nix");
    cmd.args([
        "eval",
        "--impure",
        "--raw",
        "--expr",
        r#""${(builtins.getFlake "nixpkgs").rev}-${builtins.currentSystem}""#,
    ]);

    let output = run_with_timeout(cmd, Duration::from_secs(2))?;

    if output.status.success() {
        String::from_utf8(output.stdout).ok()
    } else {
        None
    }
}

fn refresh(write_cache_after: bool, cache_key: Option<&str>) -> Vec<u8> {
    let mut cmd = Command::new("nix");
    cmd.args(["eval", "--impure", "--json", "--expr", NIX_EXPR]);

    // Use timeout to prevent hanging
    let o = match run_with_timeout(cmd, Duration::from_secs(2)) {
        Some(output) => output,
        None => {
            // Timeout occurred, return empty result (will result in exit code 1)
            return Vec::new();
        }
    };

    if !o.status.success() {
        // Command failed, return empty result instead of panicking
        return Vec::new();
    }

    if write_cache_after {
        let _ = write_cache(&o.stdout, cache_key); // best-effort
    }
    o.stdout
}

fn parse_hashes(json: &[u8]) -> HashSet<String> {
    let Ok(text) = std::str::from_utf8(json) else {
        return HashSet::new();
    };

    // Fast path: extract hashes directly from JSON array
    // Format: ["/nix/store/<hash>-...", ...]
    // Pre-allocate with estimated capacity
    let mut hashes = HashSet::with_capacity(64);
    let mut i = 0;
    let bytes = text.as_bytes();

    while i < bytes.len() {
        // Look for "/nix/store/" pattern
        if bytes.get(i..i + 11) == Some(b"/nix/store/") {
            let hash_start = i + 11;
            let hash_end = hash_start + 32;

            // Validate hash position and dash separator
            if hash_end < bytes.len()
                && bytes.get(hash_end) == Some(&b'-')
                && text.is_char_boundary(hash_start)
                && text.is_char_boundary(hash_end)
            {
                hashes.insert(text[hash_start..hash_end].to_string());
                i = hash_end;
            } else {
                i += 1;
            }
        } else {
            i += 1;
        }
    }

    hashes
}

// "/nix/store/<hash>-bash-5.3/bin" => ("<hash>", "bash")
fn hash_and_name(dir: &str) -> Option<(&str, &str)> {
    if !dir.starts_with("/nix/store/") || dir.len() < 44 || dir.as_bytes().get(43) != Some(&b'-') {
        return None;
    }
    let hash = dir.get(11..43)?;
    let rest = dir.get(44..)?;                       // after "<hash>-"
    let item = rest.split('/').next().unwrap_or(""); // "bash-5.3p3"
    let b = item.as_bytes();
    let mut cut = item.len();
    for i in 0..b.len() {
        if b[i] == b'-' && b.get(i + 1).is_some_and(|c| c.is_ascii_digit()) {
            cut = i;
            break;
        }
    }
    Some((hash, &item[..cut]))
}

// XDG cache helpers
fn cache_dir() -> PathBuf {
    if let Ok(xdg) = env::var("XDG_CACHE_HOME") {
        if !xdg.is_empty() {
            return Path::new(&xdg).join("nix-path-pkgs");
        }
    }
    Path::new(&env::var("HOME").unwrap_or_else(|_| ".".into())).join(".cache/nix-path-pkgs")
}

fn cache_file(cache_key: &str) -> PathBuf {
    cache_dir().join(format!("{}-stdenv-allowed-requisites.json", cache_key))
}

// Clean up old cache files (older than 1 day)
fn cleanup_old_cache() -> io::Result<()> {
    let dir = cache_dir();
    if !dir.exists() {
        return Ok(());
    }

    let now = SystemTime::now();
    let one_day = Duration::from_secs(86400);

    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();

        if !path.is_file() {
            continue;
        }

        if let Ok(metadata) = fs::metadata(&path) {
            if let Ok(modified) = metadata.modified() {
                if let Ok(age) = now.duration_since(modified) {
                    if age > one_day {
                        let _ = fs::remove_file(&path); // best-effort
                    }
                }
            }
        }
    }

    Ok(())
}
fn read_cache(ttl_secs: u64, cache_key: Option<&str>) -> io::Result<Option<Vec<u8>>> {
    let Some(key) = cache_key else {
        return Ok(None);
    };
    let p = cache_file(key);

    let meta = match fs::metadata(&p) {
        Ok(m) => m,
        Err(_) => return Ok(None),
    };

    if meta
        .modified()
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_some_and(|d| d <= Duration::from_secs(ttl_secs))
    {
        return Ok(Some(fs::read(&p)?));
    }

    Ok(None)
}

fn write_cache(bytes: &[u8], cache_key: Option<&str>) -> io::Result<()> {
    let Some(key) = cache_key else {
        return Ok(());
    };
    let p = cache_file(key);

    fs::create_dir_all(cache_dir())?;
    fs::write(&p, bytes)?;

    // Clean up old cache files
    let _ = cleanup_old_cache(); // best-effort

    Ok(())
}
