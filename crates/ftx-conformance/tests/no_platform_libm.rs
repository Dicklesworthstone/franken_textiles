//! Production code reaches transcendental functions only through `ftx_core::det` (plan §16: every target,
//! native and wasm, must produce the same bits; the platform libm is not correctly rounded and differs across
//! ISAs and libm versions). A raw `.sin(`, `.ln(`, `.hypot(`, ... method call in a workspace crate's `src/`
//! fails this test unless the same or the preceding line carries `// det-ok: <reason>` (a method on a type
//! that routes through `det` itself, or an oracle comparison). `.powi(` must have a literal exponent in
//! −3..=3, where every lowering rounds alike (beyond that `llvm.powi`'s operation order is unpinned: debug and
//! release differ by an ULP). Unit tests at the end of a file (after its `#[cfg(test)]`) are not scanned.
//!
//! Provenance: the rules and the method list are frankensim's `xtask` `check_libm` and `check_powi`
//! (`xtask/src/main.rs` at `7b70bc9e`, same author, MIT + OpenAI/Anthropic rider), applied here to every
//! workspace crate.

use std::path::{Path, PathBuf};

/// Transcendentals that platform libm implements without a correct-rounding guarantee (`sqrt` is IEEE
/// correctly rounded and absent on purpose).
const LIBM_METHODS: &[&str] = &[
    "cbrt", "ln", "ln_1p", "log2", "log10", "exp", "exp2", "exp_m1", "expm1", "sin", "cos",
    "sin_cos", "tan", "asin", "acos", "atan", "atan2", "sinh", "cosh", "tanh", "asinh", "acosh",
    "atanh", "hypot", "powf",
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The workspace members' directories, from the root `Cargo.toml`.
fn members() -> Vec<PathBuf> {
    let toml = std::fs::read_to_string(root().join("Cargo.toml")).unwrap();
    let line = toml
        .lines()
        .find(|l| l.trim_start().starts_with("members"))
        .unwrap();
    line.split('"')
        .skip(1)
        .step_by(2)
        .map(|m| root().join(m))
        .collect()
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

fn code_of(line: &str) -> &str {
    line.find("//").map_or(line, |i| &line[..i])
}

/// The single argument of a `.powi(` call starting at `after` (just past the parenthesis).
fn powi_arg(after: &str) -> Option<&str> {
    let mut depth = 0;
    for (i, c) in after.char_indices() {
        match c {
            '(' => depth += 1,
            ')' if depth == 0 => return Some(&after[..i]),
            ')' => depth -= 1,
            _ => {}
        }
    }
    None
}

#[test]
fn production_code_uses_det_for_transcendentals() {
    let mut violations = Vec::new();
    let mut scanned = 0;
    for crate_dir in members() {
        let mut files = Vec::new();
        rust_files(&crate_dir.join("src"), &mut files);
        files.sort();
        for f in files {
            if f.ends_with("ftx-core/src/det.rs") {
                continue; // the implementation itself
            }
            let text = std::fs::read_to_string(&f).unwrap();
            let rel = f.strip_prefix(root()).unwrap_or(&f).display().to_string();
            scanned += 1;
            let mut prev = "";
            for (n, raw) in text.lines().enumerate() {
                if raw.trim_start().starts_with("#[cfg(test)]") {
                    break;
                }
                let annotated = raw.contains("det-ok:") || prev.contains("det-ok:");
                prev = raw;
                if annotated {
                    continue;
                }
                let code = code_of(raw);
                for m in LIBM_METHODS {
                    if code.contains(&format!(".{m}(")) || code.contains(&format!("f64::{m}(")) {
                        violations.push(format!(
                            "{rel}:{}: `{m}` from the platform libm: {}",
                            n + 1,
                            raw.trim()
                        ));
                    }
                }
                let mut rest = code;
                while let Some(i) = rest.find(".powi(") {
                    let after = &rest[i + ".powi(".len()..];
                    if let Some(arg) = powi_arg(after)
                        && !arg
                            .trim()
                            .parse::<i64>()
                            .is_ok_and(|v| (-3..=3).contains(&v))
                    {
                        violations.push(format!("{rel}:{}: `.powi({arg})`: use det::powi", n + 1));
                    }
                    rest = after;
                }
            }
        }
    }
    assert!(scanned > 50, "scanned only {scanned} files");
    assert!(
        violations.is_empty(),
        "{} platform-libm calls in production code (route them through ftx_core::det, or annotate \
         `// det-ok: <reason>`):\n{}",
        violations.len(),
        violations.join("\n")
    );
}
