use faro::core::answer::Answer;
use std::path::PathBuf;
use std::process::Command;

fn write_smoke_config(name: &str) -> PathBuf {
    let fake = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-engine.sh");
    let dir = std::env::temp_dir().join("faro-cli-smoke");
    std::fs::create_dir_all(&dir).expect("temp dir is writable");
    let path = dir.join(name);
    let contents = format!(
        "language = \"en\"\nengine = \"claude\"\nvalidate_links = false\n\n[engines.claude]\nbin = \"{}\"\n",
        fake.display()
    );
    std::fs::write(&path, contents).expect("config file is writable");
    path
}

#[test]
fn print_mode_emits_a_parsable_answer_on_stdout() {
    let config = write_smoke_config("config.toml");
    let output = Command::new(env!("CARGO_BIN_EXE_faro"))
        .args(["--print", "rust async runtimes"])
        .env("FARO_CONFIG", &config)
        .output()
        .expect("binary runs");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let answer: Answer =
        serde_json::from_slice(&output.stdout).expect("stdout is a JSON answer document");
    assert_eq!(answer.title, "Rust async runtimes");
    assert!(!answer.sources.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("sub-queries"));
}

#[test]
fn print_mode_without_a_query_exits_with_usage_error() {
    let config = write_smoke_config("config-no-query.toml");
    let output = Command::new(env!("CARGO_BIN_EXE_faro"))
        .arg("--print")
        .env("FARO_CONFIG", &config)
        .output()
        .expect("binary runs");
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn unknown_mode_is_rejected_by_argument_parsing() {
    let config = write_smoke_config("config-bad-mode.toml");
    let output = Command::new(env!("CARGO_BIN_EXE_faro"))
        .args(["--print", "--mode", "casual", "query"])
        .env("FARO_CONFIG", &config)
        .output()
        .expect("binary runs");
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("casual"));
}
