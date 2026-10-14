//! 文件列表多选（单击 / Ctrl·⌘ 加选 / Shift 范围选）。

use std::collections::HashSet;
use std::path::PathBuf;

#[derive(Debug, Default, Clone)]
pub(super) struct SelectionSet {
    pub items: HashSet<PathBuf>,
    pub anchor: Option<PathBuf>,
}

impl SelectionSet {
    pub fn clear(&mut self) {
        self.items.clear();
        self.anchor = None;
    }

    pub fn primary(&self) -> Option<&PathBuf> {
        self.anchor
            .as_ref()
            .filter(|p| self.items.contains(*p))
            .or_else(|| self.items.iter().next())
    }

    pub fn contains(&self, path: &PathBuf) -> bool {
        self.items.contains(path)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn set_only(&mut self, path: PathBuf) {
        self.items.clear();
        self.items.insert(path.clone());
        self.anchor = Some(path);
    }

    /// `ctrl`：切换；`shift`：从锚点到当前；否则单选。
    pub fn apply_click(&mut self, path: PathBuf, ordered: &[PathBuf], ctrl: bool, shift: bool) {
        if shift {
            let anchor = self.anchor.clone().unwrap_or_else(|| path.clone());
            if let (Some(i0), Some(i1)) = (
                ordered.iter().position(|p| p == &anchor),
                ordered.iter().position(|p| p == &path),
            ) {
                let (a, b) = if i0 <= i1 { (i0, i1) } else { (i1, i0) };
                self.items.clear();
                for p in &ordered[a..=b] {
                    self.items.insert(p.clone());
                }
                self.anchor = Some(anchor);
                return;
            }
        }
        if ctrl {
            if self.items.contains(&path) {
                self.items.remove(&path);
                if self.anchor.as_ref() == Some(&path) {
                    self.anchor = self.items.iter().next().cloned();
                }
            } else {
                self.items.insert(path.clone());
                self.anchor = Some(path);
            }
            return;
        }
        self.set_only(path);
    }

    pub fn paths_vec(&self) -> Vec<PathBuf> {
        self.items.iter().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctrl_toggle_and_shift_range() {
        let ordered: Vec<PathBuf> = ["a", "b", "c", "d"]
            .into_iter()
            .map(PathBuf::from)
            .collect();
        let mut s = SelectionSet::default();
        s.apply_click(PathBuf::from("b"), &ordered, false, false);
        s.apply_click(PathBuf::from("d"), &ordered, false, true);
        assert_eq!(s.len(), 3);
        assert!(s.contains(&PathBuf::from("b")));
        assert!(s.contains(&PathBuf::from("c")));
        assert!(s.contains(&PathBuf::from("d")));
        s.apply_click(PathBuf::from("c"), &ordered, true, false);
        assert!(!s.contains(&PathBuf::from("c")));
    }
}
