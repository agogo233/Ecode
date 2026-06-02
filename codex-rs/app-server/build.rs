use std::process::Command;

fn main() {
    // Inject Git version info at build time
    let git_version = get_git_version();
    println!("cargo:rustc-env=CODEX_BUILD_VERSION={}", git_version);
    
    // Tell Cargo to rebuild if HEAD changes
    println!("cargo:rerun-if-changed=../../.git/HEAD");
}

fn get_git_version() -> String {
    // Try to get version from git tag
    if let Ok(output) = Command::new("git")
        .args(["describe", "--tags", "--always", "--dirty"])
        .output()
    {
        if output.status.success() {
            return String::from_utf8_lossy(&output.stdout).trim().to_string();
        }
    }
    
    // Fallback to short commit hash
    if let Ok(output) = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
    {
        if output.status.success() {
            return String::from_utf8_lossy(&output.stdout).trim().to_string();
        }
    }
    
    "unknown".to_string()
}
