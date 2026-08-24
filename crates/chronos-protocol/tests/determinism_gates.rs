//! Compile-time-adjacent determinism gates (D6, D16). CI grep lands in P9.

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    fn banned_needles() -> Vec<String> {
        vec![
            ["std", "collections", "HashMap"].join("::"),
            ["std", "collections", "HashSet"].join("::"),
            ["std", "time", "Instant"].join("::"),
            ["std", "time", "SystemTime"].join("::"),
            ["std", "fs"].join("::"),
            ["std", "net"].join("::"),
            ["std", "thread"].join("::"),
        ]
    }

    fn uncommented_line_contains(line: &str, needle: &str) -> bool {
        let trimmed = line.trim();
        if trimmed.starts_with("//") || trimmed.starts_with("//!") || trimmed.starts_with("///") {
            return false;
        }
        line.contains(needle)
    }

    fn walk_rs(dir: &Path, hits: &mut Vec<String>) {
        let entries = fs::read_dir(dir).expect("read src");
        for entry in entries {
            let entry = entry.expect("entry");
            let path = entry.path();
            if path.is_dir() {
                walk_rs(&path, hits);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let src = fs::read_to_string(&path).expect("read rs");
            for (i, line) in src.lines().enumerate() {
                for needle in banned_needles() {
                    if uncommented_line_contains(line, &needle) {
                        hits.push(format!("{}:{}: {needle}", path.display(), i + 1));
                    }
                }
            }
        }
    }

    #[test]
    fn protocol_src_has_no_host_io_or_hashmap() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut hits = Vec::new();
        walk_rs(&src, &mut hits);
        assert!(
            hits.is_empty(),
            "determinism gate failed:\n{}",
            hits.join("\n")
        );
    }
}
