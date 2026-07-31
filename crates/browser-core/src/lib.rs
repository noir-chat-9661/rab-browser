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
    pub favicon_url: Option<String>,
    pub can_go_back: bool,
    pub can_go_forward: bool,
}

impl Tab {
    fn new(id: TabId, url: impl Into<String>) -> Self {
        Self {
            id,
            url: url.into(),
            title: String::new(),
            favicon_url: None,
            can_go_back: false,
            can_go_forward: false,
        }
    }
}

/// A bookmark kept for the lifetime of the browser process.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Bookmark {
    pub url: String,
    pub title: String,
}

/// Owns the in-memory bookmark list.
#[derive(Debug, Default)]
pub struct BookmarkManager {
    bookmarks: Vec<Bookmark>,
}

impl BookmarkManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a bookmark for a new URL or removes the existing bookmark.
    ///
    /// Returns `true` when the bookmark was added and `false` when it was removed.
    pub fn toggle(&mut self, url: impl Into<String>, title: impl Into<String>) -> bool {
        let url = url.into();
        if let Some(index) = self
            .bookmarks
            .iter()
            .position(|bookmark| bookmark.url == url)
        {
            self.bookmarks.remove(index);
            false
        } else {
            self.bookmarks.push(Bookmark {
                url,
                title: title.into(),
            });
            true
        }
    }

    /// Removes a bookmark by URL. Returns `true` if it existed.
    pub fn remove(&mut self, url: &str) -> bool {
        let Some(index) = self
            .bookmarks
            .iter()
            .position(|bookmark| bookmark.url == url)
        else {
            return false;
        };
        self.bookmarks.remove(index);
        true
    }

    pub fn contains(&self, url: &str) -> bool {
        self.bookmarks.iter().any(|bookmark| bookmark.url == url)
    }

    pub fn bookmarks(&self) -> impl Iterator<Item = &Bookmark> {
        self.bookmarks.iter()
    }
}

/// Search providers supported by the browser's address bar.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SearchEngine {
    #[default]
    Google,
    DuckDuckGo,
    Bing,
}

impl SearchEngine {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Google => "google",
            Self::DuckDuckGo => "duckduckgo",
            Self::Bing => "bing",
        }
    }
}

impl std::str::FromStr for SearchEngine {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "google" => Ok(Self::Google),
            "duckduckgo" => Ok(Self::DuckDuckGo),
            "bing" => Ok(Self::Bing),
            _ => Err(()),
        }
    }
}

/// Color themes supported by the browser chrome.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Theme {
    #[default]
    Dark,
    Light,
}

impl Theme {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }
}

impl std::str::FromStr for Theme {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "dark" => Ok(Self::Dark),
            "light" => Ok(Self::Light),
            _ => Err(()),
        }
    }
}

/// Browser settings kept for the lifetime of the process.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AppSettings {
    pub search_engine: SearchEngine,
    pub theme: Theme,
}

/// One page in the browser-wide browsing history.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryEntry {
    pub url: String,
    pub title: String,
}

/// Owns the bounded in-memory browsing history.
#[derive(Debug)]
pub struct HistoryManager {
    entries: Vec<HistoryEntry>,
    capacity: usize,
}

impl Default for HistoryManager {
    fn default() -> Self {
        Self::with_capacity(200)
    }
}

impl HistoryManager {
    pub fn new() -> Self {
        Self::default()
    }

    fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: Vec::new(),
            capacity,
        }
    }

    /// Records a navigation unless it repeats the most recent URL.
    pub fn record(&mut self, url: impl Into<String>, title: impl Into<String>) {
        let url = url.into();
        if self.entries.last().is_some_and(|entry| entry.url == url) {
            return;
        }

        self.entries.push(HistoryEntry {
            url,
            title: title.into(),
        });
        if self.entries.len() > self.capacity {
            self.entries.remove(0);
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn update_latest_title(&mut self, url: &str, title: impl Into<String>) {
        if let Some(entry) = self.entries.iter_mut().rev().find(|entry| entry.url == url) {
            entry.title = title.into();
        }
    }

    pub fn entries(&self) -> impl DoubleEndedIterator<Item = &HistoryEntry> {
        self.entries.iter()
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

    #[test]
    fn toggles_bookmarks_by_url() {
        let mut bookmarks = BookmarkManager::new();

        assert!(bookmarks.toggle("https://example.com", "Example"));
        assert!(bookmarks.contains("https://example.com"));
        assert_eq!(
            bookmarks.bookmarks().collect::<Vec<_>>(),
            vec![&Bookmark {
                url: "https://example.com".to_owned(),
                title: "Example".to_owned(),
            }]
        );

        assert!(!bookmarks.toggle("https://example.com", "Updated title"));
        assert!(!bookmarks.contains("https://example.com"));
        assert_eq!(bookmarks.bookmarks().count(), 0);
    }

    #[test]
    fn app_settings_use_expected_defaults() {
        assert_eq!(AppSettings::default().search_engine, SearchEngine::Google);
        assert_eq!(AppSettings::default().theme, Theme::Dark);
    }

    #[test]
    fn history_skips_consecutive_duplicates_and_discards_old_entries() {
        let mut history = HistoryManager::with_capacity(2);

        history.record("https://example.com", "Example");
        history.record("https://example.com", "Duplicate");
        history.record("https://example.org", "Example Org");
        history.record("https://example.net", "Example Net");

        assert_eq!(
            history.entries().collect::<Vec<_>>(),
            vec![
                &HistoryEntry {
                    url: "https://example.org".to_owned(),
                    title: "Example Org".to_owned(),
                },
                &HistoryEntry {
                    url: "https://example.net".to_owned(),
                    title: "Example Net".to_owned(),
                },
            ]
        );

        history.clear();
        assert_eq!(history.entries().count(), 0);
    }
}
