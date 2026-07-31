use faro::core::answer::Answer;
use std::path::PathBuf;
use std::process::Command;

fn smoke_dir() -> PathBuf {
    let dir = std::env::temp_dir().join("faro-cli-smoke");
    std::fs::create_dir_all(&dir).expect("temp dir is writable");
    dir
}

fn write_smoke_config(name: &str) -> PathBuf {
    let fake = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-engine.sh");
    let path = smoke_dir().join(name);
    let contents = format!(
        "language = \"en\"\nengine = \"claude\"\nvalidate_links = false\n\n[engines.claude]\nbin = \"{}\"\n",
        fake.display()
    );
    std::fs::write(&path, contents).expect("config file is writable");
    path
}

fn history_path(name: &str) -> PathBuf {
    let path = smoke_dir().join(name);
    let _ = std::fs::remove_file(&path);
    path
}

fn faro(config: &PathBuf, history: &PathBuf) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_faro"));
    command
        .env("FARO_CONFIG", config)
        .env("FARO_HISTORY", history);
    command
}

#[test]
fn print_mode_emits_a_parsable_answer_on_stdout() {
    let config = write_smoke_config("config.toml");
    let history = history_path("history-print.jsonl");
    let output = faro(&config, &history)
        .args(["--print", "rust async runtimes"])
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
    let history = history_path("history-no-query.jsonl");
    let output = faro(&config, &history)
        .arg("--print")
        .output()
        .expect("binary runs");
    assert_eq!(output.status.code(), Some(2));
    assert!(!history.exists());
}

#[test]
fn unknown_mode_is_rejected_by_argument_parsing() {
    let config = write_smoke_config("config-bad-mode.toml");
    let history = history_path("history-bad-mode.jsonl");
    let output = faro(&config, &history)
        .args(["--print", "--mode", "casual", "query"])
        .output()
        .expect("binary runs");
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("casual"));
}

#[test]
fn fast_mode_prints_an_answer_and_records_it_as_fast() {
    let config = write_smoke_config("config-fast.toml");
    let history = history_path("history-fast.jsonl");
    let output = faro(&config, &history)
        .args(["--fast", "--print", "rust async runtimes"])
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
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("searching 1 sub-queries"));
    let recorded = std::fs::read_to_string(&history).expect("history file was written");
    assert_eq!(recorded.lines().count(), 1);
    assert!(recorded.contains("\"fast\":true"));
    assert!(recorded.contains("rust async runtimes"));
}

#[test]
fn searches_append_one_history_line_each() {
    let config = write_smoke_config("config-history.toml");
    let history = history_path("history-append.jsonl");
    for query in ["first query", "second query"] {
        let output = faro(&config, &history)
            .args(["--print", query])
            .output()
            .expect("binary runs");
        assert!(output.status.success());
    }
    let recorded = std::fs::read_to_string(&history).expect("history file was written");
    assert_eq!(recorded.lines().count(), 2);
    assert!(recorded.contains("first query"));
    assert!(recorded.contains("second query"));
}

#[test]
fn clear_history_erases_the_file_and_reports_the_count() {
    let config = write_smoke_config("config-clear.toml");
    let history = history_path("history-clear.jsonl");
    std::fs::write(&history, "{\"query\":\"alpha\"}\n{\"query\":\"beta\"}\n")
        .expect("history file is writable");
    let output = faro(&config, &history)
        .arg("--clear-history")
        .output()
        .expect("binary runs");
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("search history cleared (2 entries)"));
    assert!(!history.exists());
}

#[test]
fn clear_history_is_harmless_when_nothing_was_saved() {
    let config = write_smoke_config("config-clear-empty.toml");
    let history = history_path("history-clear-empty.jsonl");
    let output = faro(&config, &history)
        .arg("--clear-history")
        .output()
        .expect("binary runs");
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("search history cleared (0 entries)"));
}
