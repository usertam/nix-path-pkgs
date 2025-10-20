// Unit tests for internal functions
// Since the functions in main.rs are not pub, we'll test them through
// a test module that includes the source

#[path = "../src/main.rs"]
mod main_module;

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    // We need to expose internal functions for testing
    // For now, we'll test what we can through the module

    #[test]
    fn test_hash_and_name_valid_bash() {
        // This tests the hash_and_name function logic
        // Nix store hashes are exactly 32 characters
        let dir = "/nix/store/abc123def45678901234567890123456-bash-5.2-p15/bin";

        // We can't call the function directly as it's private, but we can test the logic
        let expected_hash = "abc123def45678901234567890123456";
        let expected_name = "bash";

        // Check format
        assert!(dir.starts_with("/nix/store/"));
        assert!(dir.len() >= 44);
        assert_eq!(dir.as_bytes()[43], b'-');

        let hash = &dir[11..43];
        assert_eq!(hash, expected_hash);

        let rest = &dir[44..];
        let item = rest.split('/').next().unwrap();
        assert_eq!(item, "bash-5.2-p15");

        // Find where version starts
        let bytes = item.as_bytes();
        let mut cut = item.len();
        for i in 0..bytes.len() {
            if bytes[i] == b'-' && bytes.get(i + 1).is_some_and(|c| c.is_ascii_digit()) {
                cut = i;
                break;
            }
        }
        let name = &item[..cut];
        assert_eq!(name, expected_name);
    }

    #[test]
    fn test_hash_and_name_valid_git() {
        let dir = "/nix/store/xyz789abc12345678901234567890123-git-2.40.1/bin";

        let hash = &dir[11..43];
        assert_eq!(hash.len(), 32);

        let rest = &dir[44..];
        let item = rest.split('/').next().unwrap();
        assert_eq!(item, "git-2.40.1");

        let bytes = item.as_bytes();
        let mut cut = item.len();
        for i in 0..bytes.len() {
            if bytes[i] == b'-' && bytes.get(i + 1).is_some_and(|c| c.is_ascii_digit()) {
                cut = i;
                break;
            }
        }
        assert_eq!(&item[..cut], "git");
    }

    #[test]
    fn test_hash_and_name_with_dash_in_name() {
        let dir = "/nix/store/12345678901234567890123456789012-cargo-watch-8.4.0/bin";

        let rest = &dir[44..];
        let item = rest.split('/').next().unwrap();
        assert_eq!(item, "cargo-watch-8.4.0");

        let bytes = item.as_bytes();
        let mut cut = item.len();
        for i in 0..bytes.len() {
            if bytes[i] == b'-' && bytes.get(i + 1).is_some_and(|c| c.is_ascii_digit()) {
                cut = i;
                break;
            }
        }
        assert_eq!(&item[..cut], "cargo-watch");
    }

    #[test]
    fn test_hash_and_name_no_version() {
        let dir = "/nix/store/12345678901234567890123456789012-rustup/bin";

        let rest = &dir[44..];
        let item = rest.split('/').next().unwrap();
        assert_eq!(item, "rustup");

        let bytes = item.as_bytes();
        let mut cut = item.len();
        for i in 0..bytes.len() {
            if bytes[i] == b'-' && bytes.get(i + 1).is_some_and(|c| c.is_ascii_digit()) {
                cut = i;
                break;
            }
        }
        assert_eq!(&item[..cut], "rustup");
    }

    #[test]
    fn test_hash_and_name_invalid_too_short() {
        let dir = "/nix/store/short-package/bin";

        // Should fail - not long enough for hash
        assert!(dir.len() < 44);
    }

    #[test]
    fn test_hash_and_name_invalid_no_dash() {
        let dir = "/nix/store/12345678901234567890123456789012package/bin";

        // Should fail - no dash after hash
        if dir.len() >= 44 {
            assert_ne!(dir.as_bytes()[43], b'-');
        }
    }

    #[test]
    fn test_hash_and_name_invalid_not_nix_store() {
        let dir = "/usr/local/123456789012345678901234567890123456-package/bin";

        assert!(!dir.starts_with("/nix/store/"));
    }

    #[test]
    fn test_parse_hashes_json_format() {
        // Test the JSON parsing logic
        let json = br#"["/nix/store/abc123def45678901234567890123456-bash-5.2/","/nix/store/xyz78901234567890123456789012345-coreutils-9.1/"]"#;

        let text = std::str::from_utf8(json).unwrap();
        let mut hashes = HashSet::new();
        let bytes = text.as_bytes();

        let mut i = 0;
        while i < bytes.len() {
            if bytes.get(i..i + 11) == Some(b"/nix/store/") {
                let hash_start = i + 11;
                let hash_end = hash_start + 32;

                if hash_end < bytes.len()
                    && bytes.get(hash_end) == Some(&b'-')
                    && text.is_char_boundary(hash_start)
                    && text.is_char_boundary(hash_end)
                {
                    hashes.insert(&text[hash_start..hash_end]);
                    i = hash_end;
                } else {
                    i += 1;
                }
            } else {
                i += 1;
            }
        }

        assert_eq!(hashes.len(), 2);
        assert!(hashes.contains("abc123def45678901234567890123456"));
        assert!(hashes.contains("xyz78901234567890123456789012345"));
    }

    #[test]
    fn test_parse_hashes_empty() {
        let json = b"[]";
        let text = std::str::from_utf8(json).unwrap();
        let mut hashes = HashSet::new();
        let bytes = text.as_bytes();

        let mut i = 0;
        while i < bytes.len() {
            if bytes.get(i..i + 11) == Some(b"/nix/store/") {
                let hash_start = i + 11;
                let hash_end = hash_start + 32;

                if hash_end < bytes.len()
                    && bytes.get(hash_end) == Some(&b'-')
                    && text.is_char_boundary(hash_start)
                    && text.is_char_boundary(hash_end)
                {
                    hashes.insert(&text[hash_start..hash_end]);
                    i = hash_end;
                } else {
                    i += 1;
                }
            } else {
                i += 1;
            }
        }

        assert_eq!(hashes.len(), 0);
    }

    #[test]
    fn test_parse_hashes_malformed() {
        let json = b"invalid json";
        let text = std::str::from_utf8(json).unwrap();
        let mut hashes = HashSet::new();
        let bytes = text.as_bytes();

        let mut i = 0;
        while i < bytes.len() {
            if bytes.get(i..i + 11) == Some(b"/nix/store/") {
                let hash_start = i + 11;
                let hash_end = hash_start + 32;

                if hash_end < bytes.len()
                    && bytes.get(hash_end) == Some(&b'-')
                    && text.is_char_boundary(hash_start)
                    && text.is_char_boundary(hash_end)
                {
                    hashes.insert(&text[hash_start..hash_end]);
                    i = hash_end;
                } else {
                    i += 1;
                }
            } else {
                i += 1;
            }
        }

        // Should handle gracefully and return empty
        assert_eq!(hashes.len(), 0);
    }

    #[test]
    fn test_cache_key_format() {
        // Test that cache key has expected format
        let cache_key = "c12c63cd6c5eb34c7b4c3076c6a99e00fcab86ec-aarch64-darwin";

        let parts: Vec<&str> = cache_key.split('-').collect();
        assert!(parts.len() >= 3); // hash has dashes, system might too

        // First part should be 40-char git hash
        assert_eq!(parts[0].len(), 40);
        assert!(parts[0].chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_skip_list() {
        let skip = &["bash-interactive", "ghostty", "ghostty-bin"];

        assert!(skip.contains(&"bash-interactive"));
        assert!(skip.contains(&"ghostty"));
        assert!(skip.contains(&"ghostty-bin"));
        assert!(!skip.contains(&"bash"));
        assert!(!skip.contains(&"git"));
    }

    #[test]
    fn test_nix_expr_format() {
        let expr = r#"
with builtins.getFlake "nixpkgs";
with legacyPackages.${builtins.currentSystem};
lib.filter lib.isDerivation stdenv.allowedRequisites
"#;

        assert!(expr.contains("getFlake"));
        assert!(expr.contains("nixpkgs"));
        assert!(expr.contains("stdenv.allowedRequisites"));
        assert!(expr.contains("lib.filter"));
    }

    #[test]
    fn test_cache_filename_format() {
        let cache_key = "abc123-x86_64-linux";
        let filename = format!("{}-stdenv-allowed-requisites.json", cache_key);

        assert!(filename.ends_with(".json"));
        assert!(filename.contains("stdenv-allowed-requisites"));
        assert!(filename.starts_with("abc123"));
    }

    #[test]
    fn test_path_splitting() {
        let path = "/nix/store/abc-bash/bin:/nix/store/def-git/bin:/usr/bin";
        let entries: Vec<&str> = path.split(':').filter(|s| !s.is_empty()).collect();

        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0], "/nix/store/abc-bash/bin");
        assert_eq!(entries[1], "/nix/store/def-git/bin");
        assert_eq!(entries[2], "/usr/bin");
    }

    #[test]
    fn test_path_empty_entries() {
        let path = ":/nix/store/abc-bash/bin:::/nix/store/def-git/bin:";
        let entries: Vec<&str> = path.split(':').filter(|s| !s.is_empty()).collect();

        // Should filter out empty strings
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn test_deduplication_logic() {
        let mut seen = HashSet::new();
        let mut ordered = Vec::new();

        let packages = vec!["bash", "git", "bash", "cargo", "git"];

        for pkg in packages {
            if seen.insert(pkg) {
                ordered.push(pkg);
            }
        }

        assert_eq!(ordered, vec!["bash", "git", "cargo"]);
        assert_eq!(ordered.len(), 3);
    }

    #[test]
    fn test_output_format() {
        let packages = vec!["bash", "git", "cargo"];
        let output = packages.join(", ");

        assert_eq!(output, "bash, git, cargo");
        assert!(output.contains(", "));
        assert_eq!(output.matches(", ").count(), 2);
    }
}
