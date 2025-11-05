use std::{
    env, fs, io,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

// XDG cache helpers
pub fn cache_dir() -> PathBuf {
    if let Ok(xdg) = env::var("XDG_CACHE_HOME") {
        if !xdg.is_empty() {
            return Path::new(&xdg).join("nix-path-pkgs");
        }
    }
    Path::new(&env::var("HOME").unwrap_or_else(|_| ".".into())).join(".cache/nix-path-pkgs")
}

pub fn cache_file(cache_key: &str) -> PathBuf {
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

pub fn read_cache(ttl_secs: u64, cache_key: Option<&str>) -> io::Result<Option<Vec<u8>>> {
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

pub fn write_cache(bytes: &[u8], cache_key: Option<&str>) -> io::Result<()> {
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

// Handle all 6 cache scenarios properly
pub fn get_or_refresh_stdenv(
    ttl: u64,
    project_rev: &Option<String>,
    system_rev: &Option<String>,
    project_key: Option<&str>,
    system_key: Option<&str>,
) -> Vec<u8> {
    match (project_rev, system_rev) {
        // Case 1-4: Both project and system exist
        (Some(proj_rev), Some(sys_rev)) if proj_rev != sys_rev => {
            // Different revisions - need both caches
            let proj_cache = read_cache(ttl, project_key).ok().flatten();
            let sys_cache = read_cache(ttl, system_key).ok().flatten();

            match (proj_cache, sys_cache) {
                // Case 2: Both cached
                (Some(proj_data), Some(sys_data)) => {
                    super::nix::merge_stdenv_json(&proj_data, &sys_data)
                }
                // Case 1: System cached, project not
                (None, Some(sys_data)) => {
                    let proj_data = super::nix::query_stdenv(proj_rev);
                    let _ = write_cache(&proj_data, project_key);
                    super::nix::merge_stdenv_json(&proj_data, &sys_data)
                }
                // Case 3: Project cached, system not
                (Some(proj_data), None) => {
                    let sys_data = super::nix::query_stdenv(sys_rev);
                    let _ = write_cache(&sys_data, system_key);
                    super::nix::merge_stdenv_json(&proj_data, &sys_data)
                }
                // Case 4: Neither cached - query both in parallel!
                (None, None) => {
                    use std::thread;

                    // Clone revisions for thread move
                    let proj_rev_clone = proj_rev.clone();
                    let sys_rev_clone = sys_rev.clone();

                    // Spawn both queries in parallel
                    let proj_handle = thread::spawn(move || {
                        super::nix::query_stdenv(&proj_rev_clone)
                    });
                    let sys_handle = thread::spawn(move || {
                        super::nix::query_stdenv(&sys_rev_clone)
                    });

                    // Wait for both to complete (~400ms instead of ~800ms!)
                    let proj_data = proj_handle.join().unwrap_or_default();
                    let sys_data = sys_handle.join().unwrap_or_default();

                    // Write both caches
                    let _ = write_cache(&proj_data, project_key);
                    let _ = write_cache(&sys_data, system_key);

                    // Merge results
                    super::nix::merge_stdenv_json(&proj_data, &sys_data)
                }
            }
        }
        // Case 5: Only system (or project == system)
        (_, Some(sys_rev)) => {
            if let Some(cached) = read_cache(ttl, system_key).ok().flatten() {
                return cached;
            }
            let data = super::nix::query_stdenv(sys_rev);
            let _ = write_cache(&data, system_key);
            data
        }
        // Case 6: Only project
        (Some(proj_rev), None) => {
            if let Some(cached) = read_cache(ttl, project_key).ok().flatten() {
                return cached;
            }
            let data = super::nix::query_stdenv(proj_rev);
            let _ = write_cache(&data, project_key);
            data
        }
        // No nixpkgs found - fallback
        (None, None) => super::nix::query_stdenv_fallback(),
    }
}
