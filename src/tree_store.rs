use crate::config_store::resolve_path_with_fallback;
use crate::core::tree::{
    ResearchTree, SESSION_VERSION, Session, encode_session, parse_session, session_dir,
    session_filename,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn resolve_dir() -> PathBuf {
    resolve_path_with_fallback("MUADDIB_SESSIONS", "XDG_STATE_HOME", session_dir)
}

pub fn save(tree: &ResearchTree, existing: Option<&Path>) -> std::io::Result<PathBuf> {
    let saved_at = unix_seconds();
    let path = existing.map_or_else(
        || resolve_dir().join(session_filename(saved_at)),
        Path::to_path_buf,
    );
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let session = Session {
        version: SESSION_VERSION,
        saved_at,
        tree: tree.clone(),
    };
    fs::write(&path, encode_session(&session))?;
    Ok(path)
}

pub fn load(path: &Path) -> Result<Session, String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("cannot read session {}: {error}", path.display()))?;
    parse_session(&text)
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::answer::Answer;
    use crate::core::mode::Mode;
    use crate::core::tree::NodeSeed;

    fn sample_tree() -> ResearchTree {
        let mut tree = ResearchTree::default();
        tree.add_node(
            None,
            NodeSeed {
                query: "root".to_string(),
                mode: Mode::Scientific,
                fast: false,
                started_at: 1,
                completed_at: 2,
                answer: Answer::default(),
                sub_queries: Vec::new(),
                web_urls: Vec::new(),
            },
        );
        tree
    }

    #[test]
    fn save_then_load_round_trips_and_overwrites_in_place() {
        let dir = std::env::temp_dir().join(format!("muaddib-tree-test-{}", std::process::id()));
        let tree = sample_tree();
        let first = save(&tree, Some(&dir.join("session.json"))).unwrap();
        let reloaded = load(&first).unwrap();
        assert_eq!(reloaded.tree, tree);
        assert_eq!(reloaded.version, SESSION_VERSION);
        let second = save(&tree, Some(&first)).unwrap();
        assert_eq!(first, second);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_reports_missing_and_garbage_files() {
        assert!(load(Path::new("/nonexistent/session.json")).is_err());
        let dir = std::env::temp_dir().join(format!("muaddib-tree-bad-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let bad = dir.join("bad.json");
        fs::write(&bad, "not json").unwrap();
        assert!(load(&bad).is_err());
        let _ = fs::remove_dir_all(&dir);
    }
}
