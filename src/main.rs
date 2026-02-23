pub mod cache;
pub mod nix;

use std::{collections::HashSet, env, process::ExitCode};

const SKIP: &[&str] = &["bash-interactive", "ghostty", "ghostty-bin", "ghostty-bin-nightly"];

fn main() -> ExitCode {
    // cache TTL (secs). TTL=0 => no cache (no read, no write).
    let ttl: u64 = env::var("NIX_PATH_PKGS_CACHE_TTL")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3600);

    // Get both project and system revisions
    let (project_rev, system_rev) = nix::get_nixpkgs_revs();

    // Generate cache keys for both (if they exist)
    let project_key = project_rev.as_ref().map(|rev| nix::make_cache_key(rev));
    let system_key = system_rev.as_ref().map(|rev| nix::make_cache_key(rev));

    // Get stdenv data (handles all 6 cases properly)
    let bytes = if ttl == 0 {
        // No caching
        nix::query_stdenv_multi(&project_rev, &system_rev)
    } else {
        cache::get_or_refresh_stdenv(
            ttl,
            &project_rev,
            &system_rev,
            project_key.as_deref(),
            system_key.as_deref(),
        )
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
