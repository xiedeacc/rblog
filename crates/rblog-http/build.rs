use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    if let Some(branch_ref) = current_branch_ref() {
        println!("cargo:rerun-if-changed=../../.git/{branch_ref}");
    }

    let rev = git(&["rev-parse", "--short=8", "HEAD"]).unwrap_or_else(|| "unknown".to_owned());
    let time = git(&[
        "show",
        "-s",
        "--format=%cd",
        "--date=format:%Y%m%d %H:%M",
        "HEAD",
    ])
    .unwrap_or_default();

    println!("cargo:rustc-env=RBLOG_GIT_REV={rev}");
    println!("cargo:rustc-env=RBLOG_GIT_TIME={time}");
}

fn current_branch_ref() -> Option<String> {
    let head = std::fs::read_to_string("../../.git/HEAD").ok()?;
    head.trim().strip_prefix("ref: ").map(str::to_owned)
}

fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}
