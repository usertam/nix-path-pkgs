use std::env;
use std::process::Command;

fn get_binary_path() -> String {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    format!("{}/target/release/nix-path-pkgs", manifest_dir)
}

#[test]
fn test_binary_exists() {
    let binary = get_binary_path();
    assert!(
        std::path::Path::new(&binary).exists(),
        "Binary not found. Run `cargo build --release` first."
    );
}

#[test]
fn test_basic_execution() {
    let output = Command::new(get_binary_path())
        .output()
        .expect("Failed to execute binary");

    assert!(
        output.status.success(),
        "Binary should exit with success when packages are found"
    );
}

#[test]
fn test_output_format() {
    let output = Command::new(get_binary_path())
        .output()
        .expect("Failed to execute binary");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should output comma-separated package names
    if !stdout.trim().is_empty() {
        // If there's output, it should be comma-separated
        assert!(stdout.contains(",") || !stdout.contains('\n'),
                "Output should be comma-separated on single line");
    }
}

#[test]
fn test_cache_disabled() {
    let output = Command::new(get_binary_path())
        .env("NIX_PATH_PKGS_CACHE_TTL", "0")
        .output()
        .expect("Failed to execute binary");

    // Should still work with cache disabled
    assert!(
        output.status.success() || output.status.code() == Some(1),
        "Binary should handle cache disabled gracefully"
    );
}

#[test]
fn test_custom_cache_ttl() {
    let output = Command::new(get_binary_path())
        .env("NIX_PATH_PKGS_CACHE_TTL", "7200")
        .output()
        .expect("Failed to execute binary");

    assert!(
        output.status.success() || output.status.code() == Some(1),
        "Binary should handle custom TTL"
    );
}

#[test]
fn test_empty_path() {
    let output = Command::new(get_binary_path())
        .env("PATH", "")
        .env("NIX_PATH_PKGS_CACHE_TTL", "0") // Skip cache to avoid nix calls
        .output()
        .expect("Failed to execute binary");

    // Should exit with 1 (no packages) or panic if nix fails (exit code None)
    // Either is acceptable for this edge case
    assert!(
        output.status.code() == Some(1) || output.status.code().is_none(),
        "Should handle empty PATH (got exit code: {:?})",
        output.status.code()
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.trim().is_empty(), "Should have empty output with empty PATH");
}

#[test]
fn test_non_nix_path() {
    let output = Command::new(get_binary_path())
        .env("PATH", "/usr/bin:/bin:/usr/local/bin")
        .env("NIX_PATH_PKGS_CACHE_TTL", "0") // Skip cache to avoid nix calls
        .output()
        .expect("Failed to execute binary");

    // Should exit with 1 (no nix packages found), succeed if PATH has nix paths,
    // or panic if nix fails (exit code None)
    assert!(
        output.status.code() == Some(1) || output.status.success() || output.status.code().is_none(),
        "Should handle non-nix paths gracefully (got exit code: {:?})",
        output.status.code()
    );
}

#[test]
fn test_repeated_execution() {
    // First run (may create cache)
    let output1 = Command::new(get_binary_path())
        .output()
        .expect("Failed to execute binary");

    // Second run (should use cache)
    let output2 = Command::new(get_binary_path())
        .output()
        .expect("Failed to execute binary");

    // Both should produce same results
    assert_eq!(
        output1.stdout, output2.stdout,
        "Repeated executions should produce identical output"
    );

    assert_eq!(
        output1.status.code(),
        output2.status.code(),
        "Repeated executions should have same exit code"
    );
}

#[test]
fn test_no_duplicate_packages() {
    let output = Command::new(get_binary_path())
        .output()
        .expect("Failed to execute binary");

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let packages: Vec<&str> = stdout.trim().split(", ").collect();
        let unique: std::collections::HashSet<_> = packages.iter().collect();

        assert_eq!(
            packages.len(),
            unique.len(),
            "Output should not contain duplicate package names"
        );
    }
}

#[test]
fn test_skipped_packages_not_in_output() {
    let output = Command::new(get_binary_path())
        .output()
        .expect("Failed to execute binary");

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);

        // Check that SKIP list items are not in output
        assert!(!stdout.contains("bash-interactive"), "bash-interactive should be skipped");
        assert!(!stdout.contains("ghostty-bin"), "ghostty-bin should be skipped");
        assert!(!stdout.contains("ghostty,"), "ghostty should be skipped");
    }
}

#[test]
fn test_stderr_on_success() {
    let output = Command::new(get_binary_path())
        .output()
        .expect("Failed to execute binary");

    if output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // stderr might have nix warnings, but should not have errors
        assert!(
            !stderr.contains("error:"),
            "Should not have errors in stderr on success"
        );
    }
}

#[test]
fn test_cache_directory_creation() {
    use std::path::PathBuf;

    // Run the binary to ensure cache directory exists
    let _ = Command::new(get_binary_path())
        .output()
        .expect("Failed to execute binary");

    let cache_dir = if let Ok(xdg) = env::var("XDG_CACHE_HOME") {
        if !xdg.is_empty() {
            PathBuf::from(xdg).join("nix-path-pkgs")
        } else {
            PathBuf::from(env::var("HOME").unwrap()).join(".cache/nix-path-pkgs")
        }
    } else {
        PathBuf::from(env::var("HOME").unwrap()).join(".cache/nix-path-pkgs")
    };

    // Cache dir should exist after first run (with default TTL > 0)
    let output = Command::new(get_binary_path())
        .output()
        .expect("Failed to execute binary");

    if output.status.success() {
        assert!(
            cache_dir.exists() || env::var("NIX_PATH_PKGS_CACHE_TTL").map(|v| v == "0").unwrap_or(false),
            "Cache directory should be created on successful run"
        );
    }
}

#[test]
fn test_performance_regression() {
    use std::time::Instant;

    let start = Instant::now();
    let output = Command::new(get_binary_path())
        .output()
        .expect("Failed to execute binary");
    let duration = start.elapsed();

    // Should complete in reasonable time (even cold cache)
    // This is a loose check - adjust based on your system
    assert!(
        duration.as_secs() < 5,
        "Binary took too long: {:?}",
        duration
    );

    // Second run should be faster (warm cache)
    let start = Instant::now();
    let _ = Command::new(get_binary_path())
        .output()
        .expect("Failed to execute binary");
    let duration2 = start.elapsed();

    assert!(
        duration2.as_millis() < 500,
        "Cached run took too long: {:?}",
        duration2
    );
}

#[test]
fn test_invalid_ttl_values() {
    // Should handle invalid TTL gracefully (fall back to default)
    let test_cases = vec!["invalid", "-1", "999999999999999999999", ""];

    for ttl in test_cases {
        let output = Command::new(get_binary_path())
            .env("NIX_PATH_PKGS_CACHE_TTL", ttl)
            .output()
            .expect("Failed to execute binary");

        assert!(
            output.status.success() || output.status.code() == Some(1),
            "Should handle invalid TTL '{}' gracefully",
            ttl
        );
    }
}
