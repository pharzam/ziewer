//! Safe path resolution inside a root folder and the Markdown/HTML tree.

use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct TreeNode {
    pub name: String,
    /// "/"-joined path from the root, no leading slash.
    pub rel_path: String,
    pub is_dir: bool,
    pub children: Vec<TreeNode>,
}

fn is_hidden(name: &str) -> bool {
    name.starts_with('.')
}

/// `.md` or `.html`, case-insensitive.
pub fn is_viewable(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("html"))
        .unwrap_or(false)
}

/// Resolve a decoded, "/"-separated relative path inside `root`.
/// Returns the canonical path, or `None` for hidden components (incl. `..`),
/// missing targets, or anything that escapes the root (e.g. via symlinks).
pub fn resolve(root: &Path, rel: &str) -> Option<PathBuf> {
    let mut joined = root.to_path_buf();
    for seg in rel.split('/').filter(|s| !s.is_empty()) {
        if is_hidden(seg) || seg.contains('\\') || seg.contains('\0') {
            return None;
        }
        joined.push(seg);
    }
    let root = root.canonicalize().ok()?;
    let target = joined.canonicalize().ok()?;
    target.starts_with(&root).then_some(target)
}

fn rel_of(root: &Path, p: &Path) -> String {
    p.strip_prefix(root)
        .unwrap_or(p)
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Children of `dir` (one level), filtered and sorted. When `recurse`, children are filled in.
/// Symlinked directories are skipped to avoid loops.
fn entries(root: &Path, dir: &Path, recurse: bool) -> Vec<TreeNode> {
    let Ok(rd) = fs::read_dir(dir) else { return vec![] };
    let mut out = Vec::new();
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if is_hidden(&name) {
            continue;
        }
        let path = e.path();
        let Ok(meta) = fs::symlink_metadata(&path) else { continue };
        let is_link = meta.file_type().is_symlink();
        let is_dir = if is_link { false } else { meta.is_dir() };
        if is_link && path.is_dir() {
            continue;
        }
        if is_dir {
            let sub = entries(root, &path, true);
            if sub.is_empty() {
                continue;
            }
            out.push(TreeNode {
                name,
                rel_path: rel_of(root, &path),
                is_dir: true,
                children: if recurse { sub } else { vec![] },
            });
        } else if is_viewable(&path) {
            out.push(TreeNode { name, rel_path: rel_of(root, &path), is_dir: false, children: vec![] });
        }
    }
    out.sort_by(|a, b| {
        b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    out
}

/// Recursive tree of viewable files and the directories containing them.
pub fn build_tree(root: &Path) -> Vec<TreeNode> {
    entries(root, root, true)
}

/// One level of `dir` (inside `root`), same filtering and sorting as the tree.
pub fn list_dir(root: &Path, dir: &Path) -> Vec<TreeNode> {
    entries(root, dir, false)
}

/// First existing of README.md, index.md, index.html.
pub fn dir_index(dir: &Path) -> Option<PathBuf> {
    ["README.md", "index.md", "index.html"].iter().map(|n| dir.join(n)).find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn fixture() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        fs::write(r.join("a.md"), "a").unwrap();
        fs::write(r.join("b.txt"), "b").unwrap();
        fs::write(r.join(".secret.md"), "s").unwrap();
        fs::create_dir_all(r.join("sub")).unwrap();
        fs::write(r.join("sub/c.HTML"), "c").unwrap();
        fs::write(r.join("sub/README.md"), "r").unwrap();
        fs::create_dir_all(r.join("empty")).unwrap();
        fs::write(r.join("empty/x.txt"), "x").unwrap();
        d
    }

    fn names(n: &[TreeNode]) -> Vec<&str> { n.iter().map(|t| t.name.as_str()).collect() }

    #[test]
    fn tree_filters_and_sorts() {
        let d = fixture();
        let t = build_tree(d.path());
        assert_eq!(names(&t), vec!["sub", "a.md"]);
        assert!(t[0].is_dir);
        assert_eq!(names(&t[0].children), vec!["c.HTML", "README.md"]);
        assert_eq!(t[0].children[0].rel_path, "sub/c.HTML");
    }

    #[test]
    fn list_dir_one_level() {
        let d = fixture();
        let l = list_dir(d.path(), d.path());
        assert_eq!(names(&l), vec!["sub", "a.md"]);
        assert!(l[0].children.is_empty());
    }

    #[test]
    fn resolve_ok() {
        let d = fixture();
        assert!(resolve(d.path(), "sub/c.HTML").is_some());
        assert!(resolve(d.path(), "").is_some());
        assert!(resolve(d.path(), "sub/").is_some());
        assert!(resolve(d.path(), "missing.md").is_none());
    }

    #[test]
    fn resolve_blocks_dotdot() {
        let d = fixture();
        assert!(resolve(d.path(), "../x").is_none());
        assert!(resolve(d.path(), "sub/../../x").is_none());
        assert!(resolve(d.path(), "sub/..").is_none());
    }

    #[test]
    fn resolve_blocks_hidden() {
        let d = fixture();
        assert!(resolve(d.path(), ".secret.md").is_none());
    }

    #[cfg(unix)]
    #[test]
    fn resolve_blocks_symlink_escape() {
        let d = fixture();
        let out = tempfile::tempdir().unwrap();
        fs::write(out.path().join("f.md"), "x").unwrap();
        std::os::unix::fs::symlink(out.path(), d.path().join("link")).unwrap();
        assert!(resolve(d.path(), "link/f.md").is_none());
    }

    #[cfg(unix)]
    #[test]
    fn tree_skips_symlink_loop() {
        let d = fixture();
        std::os::unix::fs::symlink(d.path(), d.path().join("sub/loop")).unwrap();
        let t = build_tree(d.path());
        assert!(!names(&t[0].children).contains(&"loop"));
    }

    #[test]
    fn dir_index_prefers_readme() {
        let d = fixture();
        fs::write(d.path().join("sub/index.md"), "i").unwrap();
        assert_eq!(dir_index(&d.path().join("sub")).unwrap(), d.path().join("sub/README.md"));
        assert!(dir_index(&d.path().join("empty")).is_none());
    }

    #[test]
    fn viewable_ext() {
        assert!(is_viewable(Path::new("x.MD")));
        assert!(is_viewable(Path::new("x.html")));
        assert!(!is_viewable(Path::new("x.htm.txt")));
    }
}
