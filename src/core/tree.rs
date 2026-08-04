use crate::core::answer::Answer;
use crate::core::mode::Mode;
use crate::core::plan::SubQuery;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const SESSION_VERSION: u32 = 1;
pub const MAX_NODE_WEB_URLS: usize = 30;

pub type NodeId = u64;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ResearchNode {
    pub id: NodeId,
    pub parent: Option<NodeId>,
    pub query: String,
    pub mode: Mode,
    pub fast: bool,
    pub started_at: u64,
    pub completed_at: u64,
    pub answer: Answer,
    pub sub_queries: Vec<SubQuery>,
    pub web_urls: Vec<String>,
}

impl Default for ResearchNode {
    fn default() -> Self {
        Self {
            id: 0,
            parent: None,
            query: String::new(),
            mode: Mode::General,
            fast: false,
            started_at: 0,
            completed_at: 0,
            answer: Answer::default(),
            sub_queries: Vec::new(),
            web_urls: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct NodeSeed {
    pub query: String,
    pub mode: Mode,
    pub fast: bool,
    pub started_at: u64,
    pub completed_at: u64,
    pub answer: Answer,
    pub sub_queries: Vec<SubQuery>,
    pub web_urls: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ResearchTree {
    pub nodes: Vec<ResearchNode>,
    pub next_id: NodeId,
    pub current: Option<NodeId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlatNode {
    pub id: NodeId,
    pub depth: usize,
    pub last_by_level: Vec<bool>,
}

impl ResearchTree {
    pub fn add_node(&mut self, parent: Option<NodeId>, seed: NodeSeed) -> NodeId {
        let id = self.next_id;
        self.next_id += 1;
        let parent = parent.filter(|candidate| self.node(*candidate).is_some());
        self.nodes.push(ResearchNode {
            id,
            parent,
            query: seed.query,
            mode: seed.mode,
            fast: seed.fast,
            started_at: seed.started_at,
            completed_at: seed.completed_at,
            answer: seed.answer,
            sub_queries: seed.sub_queries,
            web_urls: seed.web_urls.into_iter().take(MAX_NODE_WEB_URLS).collect(),
        });
        self.current = Some(id);
        id
    }

    pub fn node(&self, id: NodeId) -> Option<&ResearchNode> {
        self.nodes.iter().find(|node| node.id == id)
    }

    pub fn current_node(&self) -> Option<&ResearchNode> {
        self.current.and_then(|id| self.node(id))
    }

    pub fn ancestors(&self, id: NodeId) -> Vec<&ResearchNode> {
        let mut path = Vec::new();
        let mut cursor = self.node(id);
        while let Some(node) = cursor {
            if path.len() > self.nodes.len() {
                break;
            }
            path.push(node);
            cursor = node.parent.and_then(|parent| self.node(parent));
        }
        path.reverse();
        path
    }

    pub fn flatten(&self) -> Vec<FlatNode> {
        let roots = self.children_of(None);
        let mut rows = Vec::new();
        for (idx, root) in roots.iter().enumerate() {
            let mut trail = vec![idx == roots.len() - 1];
            self.flatten_into(*root, &mut trail, &mut rows);
        }
        rows
    }

    fn children_of(&self, parent: Option<NodeId>) -> Vec<NodeId> {
        self.nodes
            .iter()
            .filter(|node| node.parent == parent)
            .map(|node| node.id)
            .collect()
    }

    fn flatten_into(&self, id: NodeId, trail: &mut Vec<bool>, rows: &mut Vec<FlatNode>) {
        rows.push(FlatNode {
            id,
            depth: trail.len() - 1,
            last_by_level: trail.clone(),
        });
        let children = self.children_of(Some(id));
        for (idx, child) in children.iter().enumerate() {
            trail.push(idx == children.len() - 1);
            self.flatten_into(*child, trail, rows);
            trail.pop();
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    pub version: u32,
    pub saved_at: u64,
    pub tree: ResearchTree,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            version: SESSION_VERSION,
            saved_at: 0,
            tree: ResearchTree::default(),
        }
    }
}

pub fn encode_session(session: &Session) -> String {
    serde_json::to_string_pretty(session).unwrap_or_default()
}

pub fn parse_session(text: &str) -> Result<Session, String> {
    serde_json::from_str(text).map_err(|error| format!("invalid session file: {error}"))
}

pub fn session_dir(home: &Path, xdg_state_home: Option<&Path>) -> PathBuf {
    xdg_state_home
        .map_or_else(|| home.join(".local").join("state"), Path::to_path_buf)
        .join("muaddib")
        .join("sessions")
}

pub fn session_filename(at: u64) -> String {
    format!("session-{at}.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed(query: &str) -> NodeSeed {
        NodeSeed {
            query: query.to_string(),
            mode: Mode::Scientific,
            fast: false,
            started_at: 10,
            completed_at: 20,
            answer: Answer {
                title: format!("answer to {query}"),
                ..Answer::default()
            },
            sub_queries: vec![SubQuery {
                query: format!("{query} facet"),
                ..SubQuery::default()
            }],
            web_urls: Vec::new(),
        }
    }

    #[test]
    fn add_node_assigns_sequential_ids_and_tracks_current() {
        let mut tree = ResearchTree::default();
        let root = tree.add_node(None, seed("root"));
        let child = tree.add_node(Some(root), seed("child"));
        assert_eq!(root, 0);
        assert_eq!(child, 1);
        assert_eq!(tree.current, Some(child));
        assert_eq!(tree.node(child).unwrap().parent, Some(root));
        assert_eq!(tree.current_node().unwrap().query, "child");
    }

    #[test]
    fn add_node_with_unknown_parent_becomes_a_root() {
        let mut tree = ResearchTree::default();
        let id = tree.add_node(Some(99), seed("orphan"));
        assert_eq!(tree.node(id).unwrap().parent, None);
    }

    #[test]
    fn add_node_caps_recorded_web_urls() {
        let mut tree = ResearchTree::default();
        let many: Vec<String> = (0..50)
            .map(|idx| format!("https://u{idx}.example"))
            .collect();
        let id = tree.add_node(
            None,
            NodeSeed {
                web_urls: many,
                ..seed("root")
            },
        );
        assert_eq!(tree.node(id).unwrap().web_urls.len(), MAX_NODE_WEB_URLS);
    }

    #[test]
    fn ancestors_walk_from_the_root_to_the_node() {
        let mut tree = ResearchTree::default();
        let root = tree.add_node(None, seed("root"));
        let mid = tree.add_node(Some(root), seed("mid"));
        let leaf = tree.add_node(Some(mid), seed("leaf"));
        let queries: Vec<&str> = tree
            .ancestors(leaf)
            .iter()
            .map(|node| node.query.as_str())
            .collect();
        assert_eq!(queries, vec!["root", "mid", "leaf"]);
        assert!(tree.ancestors(99).is_empty());
    }

    #[test]
    fn flatten_orders_a_forest_depth_first_with_sibling_flags() {
        struct Case {
            name: &'static str,
            build: fn(&mut ResearchTree),
            want: Vec<(&'static str, usize, Vec<bool>)>,
        }
        let cases = [
            Case {
                name: "single root",
                build: |tree| {
                    tree.add_node(None, seed("a"));
                },
                want: vec![("a", 0, vec![true])],
            },
            Case {
                name: "root with two children",
                build: |tree| {
                    let root = tree.add_node(None, seed("a"));
                    tree.add_node(Some(root), seed("b"));
                    tree.add_node(Some(root), seed("c"));
                },
                want: vec![
                    ("a", 0, vec![true]),
                    ("b", 1, vec![true, false]),
                    ("c", 1, vec![true, true]),
                ],
            },
            Case {
                name: "nested chain",
                build: |tree| {
                    let root = tree.add_node(None, seed("a"));
                    let mid = tree.add_node(Some(root), seed("b"));
                    tree.add_node(Some(mid), seed("c"));
                },
                want: vec![
                    ("a", 0, vec![true]),
                    ("b", 1, vec![true, true]),
                    ("c", 2, vec![true, true, true]),
                ],
            },
            Case {
                name: "forest of two roots",
                build: |tree| {
                    tree.add_node(None, seed("a"));
                    tree.add_node(None, seed("b"));
                },
                want: vec![("a", 0, vec![false]), ("b", 0, vec![true])],
            },
        ];
        for case in cases {
            let mut tree = ResearchTree::default();
            (case.build)(&mut tree);
            let got: Vec<(String, usize, Vec<bool>)> = tree
                .flatten()
                .into_iter()
                .map(|row| {
                    (
                        tree.node(row.id).unwrap().query.clone(),
                        row.depth,
                        row.last_by_level,
                    )
                })
                .collect();
            let want: Vec<(String, usize, Vec<bool>)> = case
                .want
                .iter()
                .map(|(query, depth, flags)| ((*query).to_string(), *depth, flags.clone()))
                .collect();
            assert_eq!(got, want, "{}", case.name);
        }
    }

    #[test]
    fn session_round_trips_through_json() {
        let mut tree = ResearchTree::default();
        let root = tree.add_node(None, seed("root"));
        tree.add_node(Some(root), seed("child"));
        let session = Session {
            version: SESSION_VERSION,
            saved_at: 123,
            tree,
        };
        let parsed = parse_session(&encode_session(&session)).unwrap();
        assert_eq!(parsed, session);
    }

    #[test]
    fn parse_session_rejects_garbage_and_tolerates_missing_keys() {
        assert!(parse_session("not json").is_err());
        let minimal = parse_session("{}").unwrap();
        assert_eq!(minimal.version, SESSION_VERSION);
        assert!(minimal.tree.nodes.is_empty());
    }

    #[test]
    fn session_paths_follow_xdg_state() {
        assert_eq!(
            session_dir(Path::new("/home/u"), None),
            PathBuf::from("/home/u/.local/state/muaddib/sessions")
        );
        assert_eq!(
            session_dir(Path::new("/home/u"), Some(Path::new("/state"))),
            PathBuf::from("/state/muaddib/sessions")
        );
        assert_eq!(session_filename(42), "session-42.json");
    }
}
