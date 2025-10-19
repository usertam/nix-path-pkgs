use std::{
    collections::HashSet, env, fs, io,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
    time::{Duration, SystemTime},
};

const SKIP: &[&str] = &["bash-interactive", "ghostty", "ghostty-bin"];
const NIX_EXPR: &str = r#"
with builtins.getFlake "nixpkgs";
with legacyPackages.${builtins.currentSystem};
lib.filter lib.isDerivation stdenv.allowedRequisites
"#;

fn main() -> ExitCode {
    // cache TTL (secs). TTL=0 => no cache (no read, no write).
    let ttl: u64 = env::var("NIX_PATH_PKGS_CACHE_TTL")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3600);

    // nix eval output (cached unless TTL=0)
    let bytes = if ttl == 0 {
        refresh(false)
    } else {
        read_cache(ttl).ok().flatten().unwrap_or_else(|| refresh(true))
    };
    let ignore = parse_hashes(&bytes);

    // Walk $PATH in order; keep first occurrence only.
    let mut ordered: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for dir in env::var("PATH").unwrap_or_default().split(':').filter(|s| !s.is_empty()) {
        if let Some((h, name)) = hash_and_name(dir) {
            if ignore.contains(h) || SKIP.contains(&name.as_str()) || name.is_empty() {
                continue;
            }
            if seen.insert(name.clone()) {
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

fn refresh(write_cache_after: bool) -> Vec<u8> {
    let o = Command::new("nix")
        .args(["eval", "--impure", "--json", "--expr", NIX_EXPR])
        .output()
        .expect("failed to exec `nix`");
    if !o.status.success() {
        panic!("nix eval failed:\n{}", String::from_utf8_lossy(&o.stderr));
    }
    if write_cache_after {
        let _ = write_cache(&o.stdout); // best-effort
    }
    o.stdout
}

fn parse_hashes(json: &[u8]) -> HashSet<String> {
    let Ok(items) = serde_json::from_slice::<Vec<String>>(json) else {
        return HashSet::new();
    };
    items
        .into_iter()
        .filter_map(|s| store_hash(&s).map(|h| h.to_owned()))
        .collect()
}

// "/nix/store/<hash>-..." => "<hash>"
fn store_hash(p: &str) -> Option<&str> {
    if !p.starts_with("/nix/store/") || p.len() < 44 || p.as_bytes().get(43) != Some(&b'-') {
        return None;
    }
    p.get(11..43)
}

// "/nix/store/<hash>-bash-5.3/bin" => ("<hash>", "bash")
fn hash_and_name(dir: &str) -> Option<(&str, String)> {
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
    Some((hash, item[..cut].to_string()))
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
fn cache_file() -> PathBuf {
    cache_dir().join("stdenv-allowed-requisites.json")
}
fn read_cache(ttl_secs: u64) -> io::Result<Option<Vec<u8>>> {
    let p = cache_file();
    let m = match fs::metadata(&p) {
        Ok(m) => m,
        Err(_) => return Ok(None),
    };
    if m.modified()
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_some_and(|d| d <= Duration::from_secs(ttl_secs))
    {
        return fs::read(&p).map(Some);
    }
    Ok(None)
}
fn write_cache(bytes: &[u8]) -> io::Result<()> {
    fs::create_dir_all(cache_dir())?;
    fs::write(cache_file(), bytes)
}
