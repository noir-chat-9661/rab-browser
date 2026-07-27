//! GUI-independent browser state and engine operations.

use std::{collections::BTreeMap, error::Error, fmt};

/// Stable identifier assigned to a tab by [`TabManager`].
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct TabId(u64);

impl TabId {
    pub fn get(self) -> u64 {
        self.0
    }
}

/// State belonging to one browser tab.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Tab {
    pub id: TabId,
    pub url: String,
    pub title: String,
    pub can_go_back: bool,
    pub can_go_forward: bool,
}

impl Tab {
    fn new(id: TabId, url: impl Into<String>) -> Self {
        Self {
            id,
            url: url.into(),
            title: String::new(),
            can_go_back: false,
            can_go_forward: false,
        }
    }
}

/// The small engine surface needed by the first browser shell.
pub trait BrowserEngine {
    fn navigate(&mut self, url: &str) -> Result<(), BrowserError>;
    fn go_back(&mut self) -> Result<(), BrowserError>;
    fn go_forward(&mut self) -> Result<(), BrowserError>;
    fn reload(&mut self) -> Result<(), BrowserError>;
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct BrowserError {
    message: String,
}

impl BrowserError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for BrowserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for BrowserError {}

/// Owns the tabs and the currently selected tab.
#[derive(Debug, Default)]
pub struct TabManager {
    tabs: BTreeMap<TabId, Tab>,
    current: Option<TabId>,
    next_id: u64,
}

impl TabManager {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            ..Self::default()
        }
    }

    pub fn add_tab(&mut self, url: impl Into<String>) -> TabId {
        let id = TabId(self.next_id);
        self.next_id += 1;
        self.tabs.insert(id, Tab::new(id, url));
        self.current = Some(id);
        id
    }

    pub fn remove_tab(&mut self, id: TabId) -> Option<Tab> {
        let removed = self.tabs.remove(&id);
        if self.current == Some(id) {
            self.current = self.tabs.keys().next_back().copied();
        }
        removed
    }

    pub fn select_tab(&mut self, id: TabId) -> bool {
        if self.tabs.contains_key(&id) {
            self.current = Some(id);
            true
        } else {
            false
        }
    }

    pub fn current_tab(&self) -> Option<&Tab> {
        self.current.and_then(|id| self.tabs.get(&id))
    }

    pub fn current_tab_mut(&mut self) -> Option<&mut Tab> {
        self.current.and_then(|id| self.tabs.get_mut(&id))
    }

    pub fn tab(&self, id: TabId) -> Option<&Tab> {
        self.tabs.get(&id)
    }

    pub fn tab_mut(&mut self, id: TabId) -> Option<&mut Tab> {
        self.tabs.get_mut(&id)
    }

    pub fn tabs(&self) -> impl Iterator<Item = &Tab> {
        self.tabs.values()
    }

    pub fn current_id(&self) -> Option<TabId> {
        self.current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manages_current_tab_and_unique_ids() {
        let mut manager = TabManager::new();
        let first = manager.add_tab("https://example.com");
        let second = manager.add_tab("https://example.org");
        assert_ne!(first, second);
        assert_eq!(manager.current_id(), Some(second));
        assert!(manager.select_tab(first));
        assert_eq!(manager.current_tab().unwrap().url, "https://example.com");
        manager.remove_tab(first);
        assert_eq!(manager.current_id(), Some(second));
    }
}
