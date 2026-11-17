use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn release_number(tag: &str) -> Option<u32> {
    let digits = tag.strip_prefix('v')?;
    (digits.len() >= 3 && digits.bytes().all(|b| b.is_ascii_digit()))
        .then(|| digits.parse().ok())
        .flatten()
}

fn main() {
    println!("cargo:rerun-if-env-changed=DRAMAFACTORY_RELEASE_VERSION");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=Cargo.toml");
    // Watch branch and tag changes, including packed refs and worktree HEADs.
    for name in ["HEAD", "refs", "packed-refs"] {
        if let Some(path) = git(&["rev-parse", "--git-path", name]) {
            println!("cargo:rerun-if-changed={path}");
        }
    }
    let baseline = env!("CARGO_PKG_VERSION_MINOR").parse::<u32>().unwrap();
    let label = std::env::var("DRAMAFACTORY_RELEASE_VERSION").unwrap_or_else(|_| {
        let exact = git(&["tag", "--points-at", "HEAD"])
            .and_then(|tags| tags.lines().filter_map(release_number).max());
        let latest = git(&["tag", "--merged", "HEAD"])
            .and_then(|tags| tags.lines().filter_map(release_number).max());
        // Unreleased commits use the next release number; source archives retain
        // the Cargo baseline. CI's explicit release version always takes priority.
        let number = exact.unwrap_or_else(|| latest.map_or(baseline, |n| n + 1));
        format!("v{:03}", number.max(baseline))
    });
    let number = release_number(&label).expect("Expected vNNN release label");
    assert!(number > 0);
    println!("cargo:rustc-env=DF_PUBLIC_VERSION={label}");
    std::fs::write(
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("version.rs"),
        format!("pub const CURRENT: u32 = {number};\n"),
    )
    .unwrap();
}
