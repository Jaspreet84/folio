//! Reading positions and marks, kept between runs in a small JSON file.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// A place in a document, as from `Rendered::anchor_at`: the source line
/// and how many rendered lines into it.
pub type Pos = (usize, usize);

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq, Eq)]
pub struct FileState {
    pub pos: Pos,
    /// When the file was last read, in seconds since the Unix epoch.
    pub seen: u64,
    /// Marks a-z for this file.
    #[serde(default)]
    pub marks: BTreeMap<char, Pos>,
}

/// A mark A-Z, which remembers the file as well as the place.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct GlobalMark {
    pub path: String,
    pub pos: Pos,
}

#[derive(Serialize, Deserialize, Default, Debug, PartialEq, Eq)]
pub struct State {
    #[serde(default)]
    pub files: HashMap<String, FileState>,
    #[serde(default)]
    pub marks: BTreeMap<char, GlobalMark>,
}

/// Files beyond this many are forgotten, oldest first.
const MAX_FILES: usize = 500;

impl State {
    /// Read the state file. A missing or damaged file gives an empty state.
    pub fn load(path: &Path) -> State {
        std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    /// Write the entries we changed into the file on disk. Another folio may
    /// have saved since we loaded, so we merge instead of overwriting, then
    /// replace the file in one step.
    pub fn save(&self, path: &Path, files: &HashSet<String>, marks: &HashSet<char>) -> io::Result<()> {
        if files.is_empty() && marks.is_empty() {
            return Ok(());
        }
        let mut disk = State::load(path);
        for f in files {
            if let Some(s) = self.files.get(f) {
                disk.files.insert(f.clone(), s.clone());
            }
        }
        for m in marks {
            match self.marks.get(m) {
                Some(g) => disk.marks.insert(*m, g.clone()),
                None => disk.marks.remove(m),
            };
        }
        disk.prune();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension(format!("tmp{}", std::process::id()));
        std::fs::write(&tmp, serde_json::to_vec_pretty(&disk)?)?;
        std::fs::rename(&tmp, path)
    }

    fn prune(&mut self) {
        if self.files.len() <= MAX_FILES {
            return;
        }
        let mut by_age: Vec<(u64, String)> = self.files.iter().map(|(k, v)| (v.seen, k.clone())).collect();
        by_age.sort();
        for (_, key) in by_age.into_iter().take(self.files.len() - MAX_FILES) {
            self.files.remove(&key);
        }
    }
}

/// Where the state file lives: `$FOLIO_STATE_FILE`, else
/// `$XDG_STATE_HOME/folio/state.json`, else the platform's usual place.
pub fn default_path() -> Option<PathBuf> {
    let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty());
    if let Some(file) = var("FOLIO_STATE_FILE") {
        return Some(PathBuf::from(file));
    }
    if let Some(dir) = var("XDG_STATE_HOME") {
        return Some(PathBuf::from(dir).join("folio").join("state.json"));
    }
    let home = PathBuf::from(var("HOME")?);
    Some(if cfg!(target_os = "macos") {
        home.join("Library/Application Support/folio/state.json")
    } else {
        home.join(".local/state/folio/state.json")
    })
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_and_merges_with_the_file_on_disk() {
        let path = std::env::temp_dir().join(format!("folio-state-{}/state.json", std::process::id()));
        let _ = std::fs::remove_file(&path);

        // First reader saves its file and a global mark.
        let mut a = State::default();
        a.files.insert("/a.md".into(), FileState { pos: (10, 1), seen: 1, marks: BTreeMap::from([('x', (3, 0))]) });
        a.marks.insert('A', GlobalMark { path: "/a.md".into(), pos: (10, 1) });
        a.save(&path, &HashSet::from(["/a.md".to_string()]), &HashSet::from(['A'])).unwrap();

        // A second reader, loaded before that save, only touched b.md.
        let mut b = State::default();
        b.files.insert("/b.md".into(), FileState { pos: (5, 0), seen: 2, ..FileState::default() });
        b.save(&path, &HashSet::from(["/b.md".to_string()]), &HashSet::new()).unwrap();

        let disk = State::load(&path);
        assert_eq!(disk.files.len(), 2);
        assert_eq!(disk.files["/a.md"].marks[&'x'], (3, 0));
        assert_eq!(disk.marks[&'A'].pos, (10, 1));
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn forgets_the_oldest_files() {
        let mut s = State::default();
        for i in 0..(MAX_FILES + 3) {
            s.files.insert(format!("/f{i}.md"), FileState { seen: i as u64, ..FileState::default() });
        }
        s.prune();
        assert_eq!(s.files.len(), MAX_FILES);
        assert!(!s.files.contains_key("/f0.md"));
        assert!(s.files.contains_key(&format!("/f{}.md", MAX_FILES + 2)));
    }

    #[test]
    fn damaged_files_load_as_empty() {
        let path = std::env::temp_dir().join(format!("folio-bad-state-{}.json", std::process::id()));
        std::fs::write(&path, "{not json").unwrap();
        assert_eq!(State::load(&path), State::default());
        let _ = std::fs::remove_file(&path);
    }
}
