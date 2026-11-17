fn main() {
    println!("cargo:rerun-if-env-changed=DRAMAFACTORY_RELEASE_VERSION");
    let fallback = format!(
        "v{:03}",
        env!("CARGO_PKG_VERSION_MINOR").parse::<u32>().unwrap()
    );
    let label = std::env::var("DRAMAFACTORY_RELEASE_VERSION").unwrap_or(fallback);
    let digits = label
        .strip_prefix('v')
        .expect("Expected vNNN release label");
    assert!(digits.len() >= 3 && digits.bytes().all(|b| b.is_ascii_digit()));
    let number: u32 = digits.parse().expect("Invalid release version");
    assert!(number > 0);
    println!("cargo:rustc-env=DF_PUBLIC_VERSION={label}");
    std::fs::write(
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("version.rs"),
        format!("pub const CURRENT: u32 = {number};\n"),
    )
    .unwrap();
}
