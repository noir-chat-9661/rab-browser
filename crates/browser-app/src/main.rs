use std::{
    cell::Cell,
    collections::BTreeMap,
    env, fs,
    net::{Ipv4Addr, TcpListener},
    rc::Rc,
    sync::{
        Arc, Mutex,
        mpsc::{self, Sender},
    },
};

use browser_core::{
    AppSettings, BookmarkManager, BrowserEngine, HistoryManager, Locale, SearchEngine, TabId,
    TabManager, Theme,
};
use browser_engine_wry::WryEngine;
use browser_mcp_server::{DispatchError, McpHttpHandle, McpRequest, RequestDispatcher, TabInfo};
#[cfg(target_os = "macos")]
use muda::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
#[cfg(target_os = "macos")]
use objc2::{rc::Retained, runtime::AnyObject};
#[cfg(target_os = "macos")]
use objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags};
use serde::{Deserialize, Serialize};
use tao::{
    dpi::{LogicalPosition, LogicalSize},
    event::{ElementState, Event, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy},
    keyboard::{KeyCode, ModifiersState},
    window::{Window, WindowBuilder},
};
use wry::{
    PageLoadEvent, Rect, WebView, WebViewBuilder,
    http::{Request, Response, header::CONTENT_TYPE},
};

const SIDEBAR_WIDTH: f64 = 264.0;
const INTERNAL_PROTOCOL: &str = "rab";
const NEW_TAB_URL: &str = "rab://newtab/";

struct ProxyDispatcher(EventLoopProxy<McpRequest>);

impl RequestDispatcher for ProxyDispatcher {
    fn dispatch(&self, request: McpRequest) -> Result<(), DispatchError> {
        self.0
            .send_event(request)
            .map_err(|_| DispatchError::LoopClosed)
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ChromeCommand {
    ChromeReady,
    SelectTab { id: u64 },
    NewTab { url: Option<String> },
    CloseTab { id: u64 },
    CloseCurrentTab,
    Navigate { url: String },
    ContentUrlChanged { url: String },
    OpenLocation,
    GoBack,
    GoForward,
    Reload,
    ToggleSidebar,
    ToggleBookmark,
    SelectBookmark { url: String },
    RemoveBookmark { url: String },
    ClearHistory,
    ClearCookies,
    SetSearchEngine { engine: String },
    SetTheme { theme: String },
    SetMcpHttp { enabled: bool, port: u16 },
    SetLocale { locale: String },
    FaviconChanged { url: String },
    OpenDevtools,
    OpenMcpHelp,
    OpenSettings,
    PaletteOpened,
    PaletteClosed,
}

#[derive(Debug)]
enum ContentEvent {
    TitleChanged { id: TabId, title: String },
    PageLoaded { id: TabId, url: String },
    FaviconChanged { id: TabId, url: String },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChromeState<'a> {
    r#type: &'static str,
    tabs: Vec<ChromeTab<'a>>,
    current_tab_id: Option<u64>,
    bookmarks: Vec<ChromeBookmark<'a>>,
    mcp_enabled: bool,
    mcp_http: &'a McpHttpState,
    settings: ChromeSettings<'a>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct McpHttpState {
    enabled: bool,
    port: u16,
    error: Option<String>,
}

impl Default for McpHttpState {
    fn default() -> Self {
        Self {
            enabled: false,
            port: 8765,
            error: None,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChromeTab<'a> {
    id: u64,
    url: &'a str,
    title: &'a str,
    favicon_url: Option<&'a str>,
    can_go_back: bool,
    can_go_forward: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChromeBookmark<'a> {
    url: &'a str,
    title: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChromeSettings<'a> {
    search_engine: &'a str,
    theme: &'a str,
    locale: &'a str,
}

#[derive(Debug)]
struct TabHistory {
    entries: Vec<String>,
    cursor: usize,
    pending_traversal: bool,
}

impl TabHistory {
    fn new(url: String) -> Self {
        Self {
            entries: vec![url],
            cursor: 0,
            pending_traversal: false,
        }
    }

    fn record_page_load(&mut self, url: String) {
        if self.pending_traversal {
            self.pending_traversal = false;
            self.entries[self.cursor] = url;
            return;
        }

        if self.entries.get(self.cursor) == Some(&url) {
            return;
        }
        if self.entries.len() == 1 && self.cursor == 0 && is_new_tab_url(&self.entries[0]) {
            self.entries[0] = url;
            return;
        }
        self.entries.truncate(self.cursor + 1);
        self.entries.push(url);
        self.cursor = self.entries.len() - 1;
    }

    fn go_back(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        self.cursor -= 1;
        self.pending_traversal = true;
        true
    }

    fn go_forward(&mut self) -> bool {
        if self.cursor + 1 >= self.entries.len() {
            return false;
        }
        self.cursor += 1;
        self.pending_traversal = true;
        true
    }

    fn can_go_back(&self) -> bool {
        self.cursor > 0
    }

    fn can_go_forward(&self) -> bool {
        self.cursor + 1 < self.entries.len()
    }
}

fn logical_window_size(window: &Window) -> LogicalSize<f64> {
    window.inner_size().to_logical::<f64>(window.scale_factor())
}

fn chrome_bounds(window: &Window) -> Rect {
    let size = logical_window_size(window);
    Rect {
        position: LogicalPosition::new(0.0, 0.0).into(),
        size: LogicalSize::new(SIDEBAR_WIDTH.min(size.width), size.height).into(),
    }
}

fn full_window_bounds(window: &Window) -> Rect {
    Rect {
        position: LogicalPosition::new(0.0, 0.0).into(),
        size: logical_window_size(window).into(),
    }
}

fn content_bounds(window: &Window, sidebar_visible: bool) -> Rect {
    let size = logical_window_size(window);
    let sidebar_width = if sidebar_visible {
        SIDEBAR_WIDTH.min(size.width)
    } else {
        0.0
    };
    Rect {
        position: LogicalPosition::new(sidebar_width, 0.0).into(),
        size: LogicalSize::new((size.width - sidebar_width).max(0.0), size.height).into(),
    }
}

fn is_new_tab_url(url: &str) -> bool {
    url == "about:blank" || url == NEW_TAB_URL
}

fn is_only_new_tab(tabs: &TabManager, id: TabId) -> bool {
    tabs.tabs().count() == 1 && tabs.tab(id).is_some_and(|tab| is_new_tab_url(&tab.url))
}

fn current_tab_is_new(tabs: &TabManager) -> bool {
    tabs.current_tab()
        .is_some_and(|tab| is_new_tab_url(&tab.url))
}

fn chrome_html(theme: Theme) -> String {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../base-ui/dist/index.html");
    fs::read_to_string(path).unwrap_or_else(|_| {
        let (background, color) = match theme {
            Theme::Dark => ("#171816", "#e9e9e3"),
            Theme::Light => ("#f3f2eb", "#292a25"),
        };
        format!(
            "<!doctype html><body style=\"margin:0;background:{background};color:{color};font:14px sans-serif;padding:24px\">\
             base-ui is not built.<br><br>Run <code>pnpm --dir base-ui build</code>.</body>"
        )
    })
}

fn internal_page_response(request: Request<Vec<u8>>, theme: Theme) -> Response<Vec<u8>> {
    let uri = request.uri();
    let html = match (uri.host(), uri.path()) {
        (Some("newtab"), "/") => {
            let (color_scheme, background, color) = match theme {
                Theme::Dark => ("dark", "#171816", "#a2a59d"),
                Theme::Light => ("light", "#f3f2eb", "#60645a"),
            };
            format!(
                "<!doctype html><html lang=\"ja\"><head><meta charset=\"utf-8\">\
                 <title>新しいタブ</title>\
                 <style>:root{{color-scheme:{color_scheme}}}html,body{{height:100%}}\
                 body{{margin:0;display:grid;place-items:center;background:{background};\
                 color:{color};font:14px system-ui,sans-serif}}</style>\
                 </head><body>新しいタブ</body></html>"
            )
        }
        _ => {
            return Response::builder()
                .status(404)
                .header(CONTENT_TYPE, "text/plain; charset=utf-8")
                .body(b"Not Found".to_vec())
                .expect("valid internal-page 404 response");
        }
    };

    Response::builder()
        .header(CONTENT_TYPE, "text/html; charset=utf-8")
        .body(html.into_bytes())
        .expect("valid internal-page response")
}

fn normalize_url(value: &str, search_engine: SearchEngine) -> String {
    let value = value.trim();
    if value.is_empty() {
        return NEW_TAB_URL.to_owned();
    }
    if let Some(query) = value.strip_prefix('?') {
        return search_url(query.trim(), search_engine);
    }
    let has_explicit_scheme = value.contains("://")
        || value.starts_with("about:")
        || value.starts_with("data:")
        || value.starts_with("file:");
    if has_explicit_scheme && url::Url::parse(value).is_ok() {
        value.to_owned()
    } else if is_likely_domain(value) {
        format!("https://{value}")
    } else {
        search_url(value, search_engine)
    }
}

fn is_likely_domain(value: &str) -> bool {
    if value.chars().any(char::is_whitespace) {
        return false;
    }

    let Ok(url) = url::Url::parse(&format!("https://{value}")) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };

    if host.eq_ignore_ascii_case("localhost") || host.parse::<std::net::IpAddr>().is_ok() {
        return true;
    }

    let Some((domain, tld)) = host.rsplit_once('.') else {
        return false;
    };
    !domain.is_empty()
        && tld.len() >= 2
        && tld.chars().all(|character| character.is_ascii_alphabetic())
        && domain.split('.').all(|label| {
            !label.is_empty()
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '-')
        })
}

fn search_url(query: &str, search_engine: SearchEngine) -> String {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("q", query)
        .finish();
    match search_engine {
        SearchEngine::Google => format!("https://www.google.com/search?{query}"),
        SearchEngine::DuckDuckGo => format!("https://duckduckgo.com/?{query}"),
        SearchEngine::Bing => format!("https://www.bing.com/search?{query}"),
    }
}

fn primary_modifier_pressed(modifiers: ModifiersState) -> bool {
    if cfg!(target_os = "macos") {
        modifiers.super_key()
    } else {
        modifiers.control_key()
    }
}

#[cfg(target_os = "macos")]
fn install_close_tab_shortcut_monitor(
    commands_tx: Sender<String>,
    event_loop_proxy: EventLoopProxy<McpRequest>,
) -> Option<Retained<AnyObject>> {
    use std::{ptr::NonNull, ptr::null_mut};

    let handler = block2::RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
        // wry 0.55 makes child WKWebViews return NO from performKeyEquivalent:.
        // AppKit handles Cmd+W as a key equivalent before tao or DOM keydown,
        // so intercept it at NSApplication dispatch and use the normal command path.
        let event = unsafe { event.as_ref() };
        let modifiers = event.modifierFlags();
        let has_command = modifiers.contains(NSEventModifierFlags::Command);
        let has_extra_modifier = modifiers.intersects(
            NSEventModifierFlags::Control
                | NSEventModifierFlags::Option
                | NSEventModifierFlags::Shift,
        );
        let is_w = event
            .charactersIgnoringModifiers()
            .is_some_and(|key| key.to_string().eq_ignore_ascii_case("w"));

        if has_command && !has_extra_modifier && !event.isARepeat() && is_w {
            if commands_tx
                .send(r#"{"type":"close_current_tab"}"#.to_owned())
                .is_ok()
            {
                // The original key event is consumed below, so explicitly wake
                // tao's waiting run loop to drain the command channel.
                let _ = event_loop_proxy.send_event(McpRequest::Wake);
            }
            null_mut()
        } else {
            event as *const NSEvent as *mut NSEvent
        }
    });

    // SAFETY: The block returns either the original NSEvent or null to consume
    // Cmd+W, exactly as required by addLocalMonitorForEventsMatchingMask:handler:.
    unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &handler) }
}

#[cfg(target_os = "macos")]
fn install_app_menu(
    commands_tx: Sender<String>,
    event_loop_proxy: EventLoopProxy<McpRequest>,
) -> Menu {
    let about = PredefinedMenuItem::about(Some("rab-browser について"), None);
    let app_separator = PredefinedMenuItem::separator();
    let hide = PredefinedMenuItem::hide(None);
    let hide_others = PredefinedMenuItem::hide_others(None);
    let show_all = PredefinedMenuItem::show_all(None);
    let quit_separator = PredefinedMenuItem::separator();
    let quit = PredefinedMenuItem::quit(None);
    let application_menu = Submenu::with_items(
        "rab-browser",
        true,
        &[
            &about,
            &app_separator,
            &hide,
            &hide_others,
            &show_all,
            &quit_separator,
            &quit,
        ],
    )
    .expect("failed to build the application menu");

    // Without a standard Edit menu, macOS won't route Cmd+C/V/X/Z/A key
    // equivalents to the focused WKWebView the way HIG-compliant apps expect.
    let undo = PredefinedMenuItem::undo(None);
    let redo = PredefinedMenuItem::redo(None);
    let edit_separator = PredefinedMenuItem::separator();
    let cut = PredefinedMenuItem::cut(None);
    let copy = PredefinedMenuItem::copy(None);
    let paste = PredefinedMenuItem::paste(None);
    let select_all = PredefinedMenuItem::select_all(None);
    let edit_menu = Submenu::with_items(
        "Edit",
        true,
        &[
            &undo,
            &redo,
            &edit_separator,
            &cut,
            &copy,
            &paste,
            &select_all,
        ],
    )
    .expect("failed to build the Edit menu");

    let mcp_help = MenuItem::with_id("rab-browser.mcp-help", "MCPの使い方", true, None);
    let settings = MenuItem::with_id("rab-browser.settings", "設定", true, None);
    let help_menu = Submenu::with_items("Help", true, &[&mcp_help, &settings])
        .expect("failed to build the Help menu");
    let menu = Menu::with_items(&[&application_menu, &edit_menu, &help_menu])
        .expect("failed to build the menu bar");

    let mcp_help_id = mcp_help.id().clone();
    let settings_id = settings.id().clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let command = if event.id == mcp_help_id {
            Some(r#"{"type":"open_mcp_help"}"#)
        } else if event.id == settings_id {
            Some(r#"{"type":"open_settings"}"#)
        } else {
            None
        };
        if let Some(command) = command
            && commands_tx.send(command.to_owned()).is_ok()
        {
            let _ = event_loop_proxy.send_event(McpRequest::Wake);
        }
    }));
    menu.init_for_nsapp();
    menu
}

fn resolve_tab_id(tabs: &TabManager, raw_id: u64) -> Option<TabId> {
    tabs.tabs()
        .find(|tab| tab.id.get() == raw_id)
        .map(|tab| tab.id)
}

fn create_content_view(
    window: &Window,
    id: TabId,
    url: &str,
    sidebar_visible: bool,
    events_tx: &Sender<ContentEvent>,
    commands_tx: &Sender<String>,
    theme: &Rc<Cell<Theme>>,
) -> wry::Result<WryEngine> {
    let title_tx = events_tx.clone();
    let load_tx = events_tx.clone();
    let favicon_tx = events_tx.clone();
    let content_commands_tx = commands_tx.clone();
    let internal_page_theme = Rc::clone(theme);
    let bounds = content_bounds(window, sidebar_visible);
    let view = WryEngine::new_with_handlers_and_bounds_and_protocol(
        window,
        url,
        Some(bounds),
        move |title| {
            let _ = title_tx.send(ContentEvent::TitleChanged { id, title });
        },
        move |event, url| {
            if matches!(event, PageLoadEvent::Finished) {
                let _ = load_tx.send(ContentEvent::PageLoaded { id, url });
            }
        },
        move |request: Request<String>| {
            let body = request.into_body();
            if let Ok(ChromeCommand::FaviconChanged { url }) =
                serde_json::from_str::<ChromeCommand>(&body)
            {
                let _ = favicon_tx.send(ContentEvent::FaviconChanged { id, url });
            } else {
                let _ = content_commands_tx.send(body);
            }
        },
        INTERNAL_PROTOCOL,
        move |request| internal_page_response(request, internal_page_theme.get()),
    )?;
    // Keep this as a post-build correction too: the window scale or size may
    // have changed while WKWebView was being initialized.
    view.set_content_bounds(window, content_bounds(window, sidebar_visible))?;
    Ok(view)
}

#[cfg(target_os = "macos")]
fn bring_chrome_to_front(chrome: &WebView) {
    use wry::WebViewExtMacOS;

    let chrome_view = chrome.webview();
    // SAFETY: wry owns `chrome_view` for this event-loop scope, and its
    // retained superview remains alive while the tao window is alive.
    unsafe {
        if let Some(parent) = chrome_view.superview() {
            // wry 0.55.1 adds every child WKWebView with NSView::addSubview, so
            // content created later otherwise stays above a full-width palette.
            // Re-adding an existing subview moves it above its siblings.
            chrome_view.removeFromSuperview();
            parent.addSubview(&chrome_view);
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn bring_chrome_to_front(_chrome: &WebView) {}

#[allow(clippy::too_many_arguments)]
fn select_content_view(
    window: &Window,
    tabs: &mut TabManager,
    views: &mut BTreeMap<TabId, WryEngine>,
    events_tx: &Sender<ContentEvent>,
    commands_tx: &Sender<String>,
    theme: &Rc<Cell<Theme>>,
    sidebar_visible: bool,
    id: TabId,
) {
    if !tabs.select_tab(id) {
        return;
    }

    // ponytail: single-active-view suspension. Only the selected tab keeps a
    // live WKWebView; every other tab is dropped to bound memory/process
    // count to one content view regardless of how many tabs are open. This
    // loses scroll position and the WKWebView's native back/forward stack
    // for backgrounded tabs (our own TabHistory still drives our back/
    // forward buttons correctly on the current page). Upgrade to an
    // LRU-of-N cache if reload lag on switch-back proves annoying, or skip
    // suspension for the tab with audio/video playing if that matters.
    let other_ids: Vec<TabId> = views.keys().copied().filter(|&vid| vid != id).collect();
    for other_id in other_ids {
        views.remove(&other_id);
    }

    if let std::collections::btree_map::Entry::Vacant(entry) = views.entry(id) {
        let Some(url) = tabs.tab(id).map(|tab| tab.url.clone()) else {
            return;
        };
        let Ok(view) = create_content_view(
            window,
            id,
            &url,
            sidebar_visible,
            events_tx,
            commands_tx,
            theme,
        ) else {
            return;
        };
        entry.insert(view);
    }

    if let Some(view) = views.get(&id) {
        let _ = view.set_visible(true);
        let _ = view.focus();
    }
}

fn update_history_flags(tabs: &mut TabManager, histories: &BTreeMap<TabId, TabHistory>, id: TabId) {
    let Some(history) = histories.get(&id) else {
        return;
    };
    if let Some(tab) = tabs.tab_mut(id) {
        tab.can_go_back = history.can_go_back();
        tab.can_go_forward = history.can_go_forward();
    }
}

fn send_state(
    chrome: &WebView,
    tabs: &TabManager,
    bookmarks: &BookmarkManager,
    settings: &AppSettings,
    mcp_enabled: bool,
    mcp_http: &McpHttpState,
) {
    let state = ChromeState {
        r#type: "state",
        tabs: tabs
            .tabs()
            .map(|tab| ChromeTab {
                id: tab.id.get(),
                url: &tab.url,
                title: &tab.title,
                favicon_url: tab.favicon_url.as_deref(),
                can_go_back: tab.can_go_back,
                can_go_forward: tab.can_go_forward,
            })
            .collect(),
        current_tab_id: tabs.current_id().map(TabId::get),
        bookmarks: bookmarks
            .bookmarks()
            .map(|bookmark| ChromeBookmark {
                url: &bookmark.url,
                title: &bookmark.title,
            })
            .collect(),
        mcp_enabled,
        mcp_http,
        settings: ChromeSettings {
            search_engine: settings.search_engine.as_str(),
            theme: settings.theme.as_str(),
            locale: settings.locale.as_str(),
        },
    };
    if let Ok(json) = serde_json::to_string(&state) {
        let _ = chrome.evaluate_script(&format!("window.rabChrome?.receive({json});"));
    }
}

#[allow(clippy::too_many_arguments)]
fn add_tab(
    window: &Window,
    tabs: &mut TabManager,
    views: &mut BTreeMap<TabId, WryEngine>,
    histories: &mut BTreeMap<TabId, TabHistory>,
    events_tx: &Sender<ContentEvent>,
    commands_tx: &Sender<String>,
    theme: &Rc<Cell<Theme>>,
    url: &str,
    sidebar_visible: bool,
    search_engine: SearchEngine,
) -> wry::Result<TabId> {
    let url = normalize_url(url, search_engine);
    let id = tabs.add_tab(url.clone());
    match create_content_view(
        window,
        id,
        &url,
        sidebar_visible,
        events_tx,
        commands_tx,
        theme,
    ) {
        Ok(view) => {
            histories.insert(id, TabHistory::new(url));
            views.insert(id, view);
            select_content_view(
                window,
                tabs,
                views,
                events_tx,
                commands_tx,
                theme,
                sidebar_visible,
                id,
            );
            Ok(id)
        }
        Err(error) => {
            tabs.remove_tab(id);
            Err(error)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn close_tab(
    window: &Window,
    tabs: &mut TabManager,
    views: &mut BTreeMap<TabId, WryEngine>,
    histories: &mut BTreeMap<TabId, TabHistory>,
    events_tx: &Sender<ContentEvent>,
    commands_tx: &Sender<String>,
    theme: &Rc<Cell<Theme>>,
    id: TabId,
    sidebar_visible: bool,
    search_engine: SearchEngine,
) -> CloseTabResult {
    if is_only_new_tab(tabs, id) {
        return CloseTabResult::Ignored;
    }

    views.remove(&id);
    histories.remove(&id);
    tabs.remove_tab(id);

    if tabs.current_id().is_none() {
        return if add_tab(
            window,
            tabs,
            views,
            histories,
            events_tx,
            commands_tx,
            theme,
            NEW_TAB_URL,
            sidebar_visible,
            search_engine,
        )
        .is_ok()
        {
            CloseTabResult::CreatedReplacement
        } else {
            CloseTabResult::Closed
        };
    } else if let Some(current) = tabs.current_id() {
        select_content_view(
            window,
            tabs,
            views,
            events_tx,
            commands_tx,
            theme,
            sidebar_visible,
            current,
        );
    }
    CloseTabResult::Closed
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CloseTabResult {
    Ignored,
    Closed,
    CreatedReplacement,
}

fn apply_layout(
    window: &Window,
    chrome: &WebView,
    views: &BTreeMap<TabId, WryEngine>,
    sidebar_visible: bool,
    palette_open: bool,
) {
    let content_rect = content_bounds(window, sidebar_visible);
    for view in views.values() {
        let _ = view.set_content_bounds(window, content_rect);
    }

    let chrome_visible = sidebar_visible || palette_open;
    let chrome_rect = if palette_open {
        full_window_bounds(window)
    } else {
        chrome_bounds(window)
    };
    let _ = chrome.set_bounds(chrome_rect);
    let _ = chrome.set_visible(chrome_visible);
}

fn focus_location(window: &Window, chrome: &WebView, palette_open: &mut bool) {
    *palette_open = true;
    let _ = chrome.set_visible(true);
    let _ = chrome.set_bounds(full_window_bounds(window));
    bring_chrome_to_front(chrome);
    let _ = chrome.focus();
    let _ = chrome.evaluate_script("window.rabChrome?.openLocation();");
}

fn open_settings(window: &Window, chrome: &WebView, palette_open: &mut bool) {
    *palette_open = true;
    let _ = chrome.set_visible(true);
    let _ = chrome.set_bounds(full_window_bounds(window));
    bring_chrome_to_front(chrome);
    let _ = chrome.focus();
    let _ = chrome.evaluate_script("window.rabChrome?.openSettings();");
}

fn open_mcp_help(window: &Window, chrome: &WebView, palette_open: &mut bool) {
    *palette_open = true;
    let _ = chrome.set_visible(true);
    let _ = chrome.set_bounds(full_window_bounds(window));
    bring_chrome_to_front(chrome);
    let _ = chrome.focus();
    let _ = chrome.evaluate_script("window.rabChrome?.openMcpHelp();");
}

fn mcp_env_enabled() -> bool {
    env::var("RAB_MCP").is_ok_and(|value| {
        !matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "" | "0" | "false" | "no" | "off"
        )
    })
}

fn parse_startup_args(args: impl IntoIterator<Item = String>) -> (bool, String) {
    let mut mcp_enabled = mcp_env_enabled();
    let mut initial_url = None;
    for argument in args {
        if argument == "--mcp" {
            mcp_enabled = true;
        } else if initial_url.is_none() {
            initial_url = Some(argument);
        }
    }
    (
        mcp_enabled,
        initial_url.unwrap_or_else(|| NEW_TAB_URL.to_owned()),
    )
}

fn eval_result(raw: String) -> Result<String, String> {
    let envelope = serde_json::from_str::<serde_json::Value>(&raw)
        .map_err(|error| format!("invalid evaluation result: {error}"))?;
    let Some(envelope) = envelope.as_object() else {
        return Err("invalid evaluation result: expected an object".to_owned());
    };
    let Some(ok) = envelope.get("ok").and_then(serde_json::Value::as_bool) else {
        return Err("invalid evaluation result: missing `ok`".to_owned());
    };

    if ok {
        let value = envelope
            .get("value")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        Ok(match value {
            serde_json::Value::String(value) => value,
            other => other.to_string(),
        })
    } else {
        Err(envelope
            .get("error")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| "evaluation failed without an error message".to_owned()))
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_mcp_request(
    request: McpRequest,
    window: &Window,
    chrome: &WebView,
    tabs: &mut TabManager,
    bookmarks: &BookmarkManager,
    settings: &AppSettings,
    views: &mut BTreeMap<TabId, WryEngine>,
    histories: &mut BTreeMap<TabId, TabHistory>,
    content_events_tx: &Sender<ContentEvent>,
    commands_tx: &Sender<String>,
    theme: &Rc<Cell<Theme>>,
    sidebar_visible: bool,
    mcp_enabled: bool,
    mcp_http_state: &McpHttpState,
) {
    match request {
        McpRequest::Wake => {}
        McpRequest::ListTabs { reply } => {
            let current = tabs.current_id();
            let tab_info = tabs
                .tabs()
                .map(|tab| TabInfo {
                    id: tab.id.get(),
                    url: tab.url.clone(),
                    title: tab.title.clone(),
                    active: current == Some(tab.id),
                })
                .collect();
            let _ = reply.send(tab_info);
        }
        McpRequest::NewTab { url, reply } => {
            match add_tab(
                window,
                tabs,
                views,
                histories,
                content_events_tx,
                commands_tx,
                theme,
                url.as_deref().unwrap_or(NEW_TAB_URL),
                sidebar_visible,
                settings.search_engine,
            ) {
                Ok(id) => {
                    bring_chrome_to_front(chrome);
                    send_state(
                        chrome,
                        tabs,
                        bookmarks,
                        settings,
                        mcp_enabled,
                        mcp_http_state,
                    );
                    let _ = reply.send(id.get());
                }
                Err(error) => {
                    eprintln!("failed to create MCP-requested tab: {error}");
                    let _ = reply.send(0);
                }
            }
        }
        McpRequest::CloseTab { id, reply } => {
            let Some(id) = resolve_tab_id(tabs, id) else {
                let _ = reply.send(false);
                return;
            };
            let result = close_tab(
                window,
                tabs,
                views,
                histories,
                content_events_tx,
                commands_tx,
                theme,
                id,
                sidebar_visible,
                settings.search_engine,
            );
            if result == CloseTabResult::CreatedReplacement {
                bring_chrome_to_front(chrome);
            }
            send_state(
                chrome,
                tabs,
                bookmarks,
                settings,
                mcp_enabled,
                mcp_http_state,
            );
            let _ = reply.send(result != CloseTabResult::Ignored);
        }
        McpRequest::SelectTab { id, reply } => {
            let selected = resolve_tab_id(tabs, id).is_some_and(|id| {
                select_content_view(
                    window,
                    tabs,
                    views,
                    content_events_tx,
                    commands_tx,
                    theme,
                    sidebar_visible,
                    id,
                );
                true
            });
            if selected {
                send_state(
                    chrome,
                    tabs,
                    bookmarks,
                    settings,
                    mcp_enabled,
                    mcp_http_state,
                );
            }
            let _ = reply.send(selected);
        }
        McpRequest::Navigate { url, reply } => {
            let Some(id) = tabs.current_id() else {
                let _ = reply.send(Err("no active tab".to_owned()));
                return;
            };
            let url = normalize_url(&url, settings.search_engine);
            let leaving_new_tab = tabs.tab(id).is_some_and(|tab| is_new_tab_url(&tab.url));
            let result = views
                .get_mut(&id)
                .ok_or_else(|| "active tab has no content view".to_owned())
                .and_then(|view| {
                    if leaving_new_tab {
                        view.navigate_replacing(&url)
                    } else {
                        view.navigate(&url)
                    }
                    .map_err(|error| error.to_string())
                });
            if result.is_ok() {
                if let Some(tab) = tabs.tab_mut(id) {
                    tab.url = url;
                    tab.favicon_url = None;
                }
                send_state(
                    chrome,
                    tabs,
                    bookmarks,
                    settings,
                    mcp_enabled,
                    mcp_http_state,
                );
            }
            let _ = reply.send(result);
        }
        McpRequest::GoBack { reply } => {
            let moved = tabs.current_id().is_some_and(|id| {
                let moved = histories.get_mut(&id).is_some_and(TabHistory::go_back);
                if moved {
                    if let Some(view) = views.get_mut(&id) {
                        let _ = view.go_back();
                    }
                    update_history_flags(tabs, histories, id);
                    send_state(
                        chrome,
                        tabs,
                        bookmarks,
                        settings,
                        mcp_enabled,
                        mcp_http_state,
                    );
                }
                moved
            });
            let _ = reply.send(moved);
        }
        McpRequest::GoForward { reply } => {
            let moved = tabs.current_id().is_some_and(|id| {
                let moved = histories.get_mut(&id).is_some_and(TabHistory::go_forward);
                if moved {
                    if let Some(view) = views.get_mut(&id) {
                        let _ = view.go_forward();
                    }
                    update_history_flags(tabs, histories, id);
                    send_state(
                        chrome,
                        tabs,
                        bookmarks,
                        settings,
                        mcp_enabled,
                        mcp_http_state,
                    );
                }
                moved
            });
            let _ = reply.send(moved);
        }
        McpRequest::Reload { reply } => {
            if current_tab_is_new(tabs) {
                let _ = reply.send(Ok(()));
                return;
            }
            let result = tabs
                .current_id()
                .ok_or_else(|| "no active tab".to_owned())
                .and_then(|id| {
                    views
                        .get_mut(&id)
                        .ok_or_else(|| "active tab has no content view".to_owned())
                })
                .and_then(|view| view.reload().map_err(|error| error.to_string()));
            let _ = reply.send(result);
        }
        McpRequest::Eval {
            target,
            script,
            reply,
        } => {
            let id = target
                .and_then(|id| resolve_tab_id(tabs, id))
                .or_else(|| target.is_none().then(|| tabs.current_id()).flatten());
            let Some(id) = id else {
                let message = if target.is_some() {
                    "target tab not found"
                } else {
                    "no active tab"
                };
                let _ = reply.send(Err(message.to_owned()));
                return;
            };
            let Some(view) = views.get(&id) else {
                let _ = reply.send(Err("target tab has no content view".to_owned()));
                return;
            };

            let pending_reply = Arc::new(Mutex::new(Some(reply)));
            let callback_reply = Arc::clone(&pending_reply);
            let evaluate_result = view.evaluate_script_with_callback(&script, move |raw| {
                if let Ok(mut reply) = callback_reply.lock()
                    && let Some(reply) = reply.take()
                {
                    let _ = reply.send(eval_result(raw));
                }
            });
            if let Err(error) = evaluate_result
                && let Ok(mut reply) = pending_reply.lock()
                && let Some(reply) = reply.take()
            {
                let _ = reply.send(Err(error.to_string()));
            }
        }
    }
}

fn main() -> wry::Result<()> {
    let (mcp_enabled, initial_url) = parse_startup_args(env::args().skip(1));
    let event_loop = EventLoopBuilder::<McpRequest>::with_user_event().build();
    let window = WindowBuilder::new()
        .with_title("rab-browser")
        .with_inner_size(LogicalSize::new(1180.0, 760.0))
        .with_min_inner_size(LogicalSize::new(620.0, 420.0))
        .build(&event_loop)
        .expect("failed to create tao window");

    let (content_events_tx, content_events_rx) = mpsc::channel::<ContentEvent>();
    let (commands_tx, commands_rx) = mpsc::channel::<String>();
    #[cfg(target_os = "macos")]
    let app_menu = install_app_menu(commands_tx.clone(), event_loop.create_proxy());
    #[cfg(target_os = "macos")]
    let close_tab_shortcut_monitor =
        install_close_tab_shortcut_monitor(commands_tx.clone(), event_loop.create_proxy())
            .expect("failed to install the macOS Cmd+W event monitor");
    let mut tabs = TabManager::new();
    let mut bookmarks = BookmarkManager::new();
    let mut history = HistoryManager::new();
    let mut settings = AppSettings::default();
    let current_theme = Rc::new(Cell::new(settings.theme));
    let mut views = BTreeMap::new();
    let mut histories = BTreeMap::new();
    let mut sidebar_visible = true;
    add_tab(
        &window,
        &mut tabs,
        &mut views,
        &mut histories,
        &content_events_tx,
        &commands_tx,
        &current_theme,
        &initial_url,
        sidebar_visible,
        settings.search_engine,
    )?;

    let chrome_commands_tx = commands_tx.clone();
    let chrome = WebViewBuilder::new()
        .with_html(chrome_html(settings.theme))
        .with_transparent(true)
        .with_devtools(true)
        .with_ipc_handler(move |request: Request<String>| {
            let _ = chrome_commands_tx.send(request.into_body());
        })
        .build_as_child(&window)?;
    let _chrome_ui_delegate = browser_engine_wry::install_js_dialog_delegate(&chrome);
    chrome.set_bounds(chrome_bounds(&window))?;

    let dispatcher: Arc<dyn RequestDispatcher> =
        Arc::new(ProxyDispatcher(event_loop.create_proxy()));
    if mcp_enabled {
        browser_mcp_server::spawn(Arc::clone(&dispatcher));
        eprintln!("rab-browser MCP server enabled on stdio");
    }

    let mut modifiers = ModifiersState::empty();
    let mut palette_open = false;
    let mut mcp_http: Option<McpHttpHandle> = None;
    let mut mcp_http_state = McpHttpState::default();
    event_loop.run(move |event, _, control_flow| {
        #[cfg(target_os = "macos")]
        // Keep the monitor token with the event loop so its registration has the
        // same explicit lifetime as the AppKit application.
        let _keep_close_tab_shortcut_monitor_alive = &close_tab_shortcut_monitor;
        #[cfg(target_os = "macos")]
        let _keep_app_menu_alive = &app_menu;

        *control_flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(request) => handle_mcp_request(
                request,
                &window,
                &chrome,
                &mut tabs,
                &bookmarks,
                &settings,
                &mut views,
                &mut histories,
                &content_events_tx,
                &commands_tx,
                &current_theme,
                sidebar_visible,
                mcp_enabled,
                &mcp_http_state,
            ),
            Event::MainEventsCleared => {
                for event in content_events_rx.try_iter() {
                    match event {
                        ContentEvent::TitleChanged { id, title } => {
                            if let Some(tab) = tabs.tab_mut(id) {
                                tab.title = title.clone();
                                if !is_new_tab_url(&tab.url) {
                                    history.update_latest_title(&tab.url, title);
                                }
                            }
                        }
                        ContentEvent::PageLoaded { id, url } => {
                            if let Some(tab) = tabs.tab_mut(id) {
                                tab.url = url.clone();
                                tab.favicon_url = None;
                            }
                            if !is_new_tab_url(&url) {
                                let title = tabs
                                    .tab(id)
                                    .map(|tab| tab.title.clone())
                                    .unwrap_or_default();
                                history.record(url.clone(), title);
                            }
                            if let Some(history) = histories.get_mut(&id) {
                                history.record_page_load(url);
                            }
                            update_history_flags(&mut tabs, &histories, id);
                        }
                        ContentEvent::FaviconChanged { id, url } => {
                            if let Some(tab) = tabs.tab_mut(id) {
                                tab.favicon_url = (!url.is_empty()).then_some(url);
                            }
                        }
                    }
                    send_state(
                        &chrome,
                        &tabs,
                        &bookmarks,
                        &settings,
                        mcp_enabled,
                        &mcp_http_state,
                    );
                }

                for raw_command in commands_rx.try_iter() {
                    let Ok(command) = serde_json::from_str::<ChromeCommand>(&raw_command) else {
                        continue;
                    };
                    match command {
                        ChromeCommand::ChromeReady => {
                            if current_tab_is_new(&tabs) {
                                focus_location(&window, &chrome, &mut palette_open);
                            }
                        }
                        ChromeCommand::SelectTab { id } => {
                            if let Some(id) = resolve_tab_id(&tabs, id) {
                                select_content_view(
                                    &window,
                                    &mut tabs,
                                    &mut views,
                                    &content_events_tx,
                                    &commands_tx,
                                    &current_theme,
                                    sidebar_visible,
                                    id,
                                );
                            }
                        }
                        ChromeCommand::NewTab { url } => {
                            if add_tab(
                                &window,
                                &mut tabs,
                                &mut views,
                                &mut histories,
                                &content_events_tx,
                                &commands_tx,
                                &current_theme,
                                url.as_deref().unwrap_or(NEW_TAB_URL),
                                sidebar_visible,
                                settings.search_engine,
                            )
                            .is_ok()
                            {
                                bring_chrome_to_front(&chrome);
                                send_state(
                                    &chrome,
                                    &tabs,
                                    &bookmarks,
                                    &settings,
                                    mcp_enabled,
                                    &mcp_http_state,
                                );
                                focus_location(&window, &chrome, &mut palette_open);
                            }
                        }
                        ChromeCommand::CloseTab { id } => {
                            if let Some(id) = resolve_tab_id(&tabs, id) {
                                let result = close_tab(
                                    &window,
                                    &mut tabs,
                                    &mut views,
                                    &mut histories,
                                    &content_events_tx,
                                    &commands_tx,
                                    &current_theme,
                                    id,
                                    sidebar_visible,
                                    settings.search_engine,
                                );
                                if result == CloseTabResult::CreatedReplacement {
                                    bring_chrome_to_front(&chrome);
                                }
                            }
                        }
                        ChromeCommand::CloseCurrentTab => {
                            if let Some(id) = tabs.current_id() {
                                let result = close_tab(
                                    &window,
                                    &mut tabs,
                                    &mut views,
                                    &mut histories,
                                    &content_events_tx,
                                    &commands_tx,
                                    &current_theme,
                                    id,
                                    sidebar_visible,
                                    settings.search_engine,
                                );
                                if result == CloseTabResult::CreatedReplacement {
                                    bring_chrome_to_front(&chrome);
                                }
                            }
                        }
                        ChromeCommand::Navigate { url } => {
                            if let Some(id) = tabs.current_id() {
                                let url = normalize_url(&url, settings.search_engine);
                                let leaving_new_tab =
                                    tabs.tab(id).is_some_and(|tab| is_new_tab_url(&tab.url));
                                let navigated = views.get_mut(&id).is_some_and(|view| {
                                    if leaving_new_tab {
                                        view.navigate_replacing(&url)
                                    } else {
                                        view.navigate(&url)
                                    }
                                    .is_ok()
                                });
                                if navigated
                                    && let Some(tab) = tabs.tab_mut(id)
                                {
                                    tab.url = url;
                                    tab.favicon_url = None;
                                }
                            }
                        }
                        ChromeCommand::ContentUrlChanged { url } => {
                            if let Some(id) = tabs.current_id() {
                                if let Some(tab) = tabs.tab_mut(id) {
                                    tab.url = url.clone();
                                }
                                if !is_new_tab_url(&url) {
                                    let title = tabs
                                        .tab(id)
                                        .map(|tab| tab.title.clone())
                                        .unwrap_or_default();
                                    history.record(url.clone(), title);
                                }
                                if let Some(history) = histories.get_mut(&id) {
                                    history.record_page_load(url);
                                }
                                update_history_flags(&mut tabs, &histories, id);
                            }
                        }
                        ChromeCommand::OpenLocation => {
                            focus_location(&window, &chrome, &mut palette_open);
                        }
                        ChromeCommand::GoBack => {
                            if let Some(id) = tabs.current_id()
                                && let Some(history) = histories.get_mut(&id)
                                && history.go_back()
                            {
                                if let Some(view) = views.get_mut(&id) {
                                    let _ = view.go_back();
                                }
                                update_history_flags(&mut tabs, &histories, id);
                            }
                        }
                        ChromeCommand::GoForward => {
                            if let Some(id) = tabs.current_id()
                                && let Some(history) = histories.get_mut(&id)
                                && history.go_forward()
                            {
                                if let Some(view) = views.get_mut(&id) {
                                    let _ = view.go_forward();
                                }
                                update_history_flags(&mut tabs, &histories, id);
                            }
                        }
                        ChromeCommand::Reload => {
                            if !current_tab_is_new(&tabs)
                                && let Some(view) =
                                    tabs.current_id().and_then(|id| views.get_mut(&id))
                            {
                                let _ = view.reload();
                            }
                        }
                        ChromeCommand::ToggleSidebar => {
                            sidebar_visible = !sidebar_visible;
                            apply_layout(&window, &chrome, &views, sidebar_visible, palette_open);
                            if sidebar_visible || palette_open {
                                bring_chrome_to_front(&chrome);
                            } else if let Some(view) =
                                tabs.current_id().and_then(|id| views.get(&id))
                            {
                                let _ = view.focus();
                            }
                        }
                        ChromeCommand::ToggleBookmark => {
                            if let Some(tab) =
                                tabs.current_tab().filter(|tab| !is_new_tab_url(&tab.url))
                            {
                                let title = if tab.title.trim().is_empty() {
                                    tab.url.clone()
                                } else {
                                    tab.title.clone()
                                };
                                bookmarks.toggle(tab.url.clone(), title);
                            }
                        }
                        ChromeCommand::SelectBookmark { url } => {
                            if let Some(id) = tabs.current_id() {
                                let url = normalize_url(&url, settings.search_engine);
                                let leaving_new_tab =
                                    tabs.tab(id).is_some_and(|tab| is_new_tab_url(&tab.url));
                                let navigated = views.get_mut(&id).is_some_and(|view| {
                                    if leaving_new_tab {
                                        view.navigate_replacing(&url)
                                    } else {
                                        view.navigate(&url)
                                    }
                                    .is_ok()
                                });
                                if navigated
                                    && let Some(tab) = tabs.tab_mut(id)
                                {
                                    tab.url = url;
                                    tab.favicon_url = None;
                                }
                            }
                        }
                        ChromeCommand::RemoveBookmark { url } => {
                            bookmarks.remove(&url);
                        }
                        ChromeCommand::ClearHistory => {
                            history.clear();
                        }
                        ChromeCommand::ClearCookies => {
                            // WKWebView instances share the default website data
                            // store unless configured otherwise, so clearing via
                            // the chrome webview affects all tabs and doesn't
                            // depend on a content tab being open.
                            if let Err(error) = chrome.clear_all_browsing_data() {
                                eprintln!("failed to clear browsing data: {error}");
                            }
                        }
                        ChromeCommand::SetSearchEngine { engine } => {
                            if let Ok(engine) = engine.parse() {
                                settings.search_engine = engine;
                            }
                        }
                        ChromeCommand::SetTheme { theme } => {
                            if let Ok(theme) = theme.parse::<Theme>() {
                                settings.theme = theme;
                                current_theme.set(theme);
                            }
                        }
                        ChromeCommand::SetMcpHttp { enabled, port } => {
                            if let Some(handle) = mcp_http.take() {
                                // Graceful shutdown waits for in-flight requests to finish,
                                // which can take a moment; run it off the GUI event loop
                                // thread so the UI doesn't freeze while it completes.
                                std::thread::spawn(move || handle.shutdown());
                            }
                            mcp_http_state = McpHttpState {
                                enabled: false,
                                port,
                                error: None,
                            };
                            if enabled {
                                match TcpListener::bind((Ipv4Addr::LOCALHOST, port)).and_then(
                                    |listener| {
                                        listener.set_nonblocking(true)?;
                                        Ok(listener)
                                    },
                                ) {
                                    Ok(listener) => {
                                        mcp_http = Some(browser_mcp_server::spawn_http(
                                            Arc::clone(&dispatcher),
                                            listener,
                                        ));
                                        mcp_http_state.enabled = true;
                                        eprintln!(
                                            "rab-browser MCP server enabled at http://127.0.0.1:{port}/mcp"
                                        );
                                    }
                                    Err(error) => {
                                        mcp_http_state.error = Some(format!(
                                            "failed to listen on 127.0.0.1:{port}: {error}"
                                        ));
                                    }
                                }
                            }
                        }
                        ChromeCommand::SetLocale { locale } => {
                            if let Ok(locale) = locale.parse::<Locale>() {
                                settings.locale = locale;
                            }
                        }
                        // Content-originated favicon_changed messages are intercepted and
                        // rerouted to ContentEvent::FaviconChanged (with the correct tab id)
                        // inside create_content_view's IPC handler, never reaching this loop.
                        // Chrome itself never sends this command. Kept only for match
                        // exhaustiveness.
                        ChromeCommand::FaviconChanged { .. } => {}
                        ChromeCommand::OpenDevtools => {
                            if let Some(view) = tabs.current_id().and_then(|id| views.get(&id)) {
                                view.open_devtools();
                            }
                        }
                        ChromeCommand::OpenMcpHelp => {
                            open_mcp_help(&window, &chrome, &mut palette_open);
                        }
                        ChromeCommand::OpenSettings => {
                            open_settings(&window, &chrome, &mut palette_open);
                        }
                        ChromeCommand::PaletteOpened => {
                            palette_open = true;
                            apply_layout(&window, &chrome, &views, sidebar_visible, palette_open);
                            bring_chrome_to_front(&chrome);
                            let _ = chrome.focus();
                        }
                        ChromeCommand::PaletteClosed => {
                            palette_open = false;
                            apply_layout(&window, &chrome, &views, sidebar_visible, palette_open);
                            if let Some(view) = tabs.current_id().and_then(|id| views.get(&id)) {
                                let _ = view.focus();
                            }
                        }
                    }
                    send_state(
                        &chrome,
                        &tabs,
                        &bookmarks,
                        &settings,
                        mcp_enabled,
                        &mcp_http_state,
                    );
                }
            }
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::Resized(_) => {
                    apply_layout(&window, &chrome, &views, sidebar_visible, palette_open);
                }
                WindowEvent::ModifiersChanged(state) => modifiers = state,
                WindowEvent::KeyboardInput { event, .. }
                    if event.state == ElementState::Pressed
                        && event.physical_key == KeyCode::F12 =>
                {
                    if let Some(view) = tabs.current_id().and_then(|id| views.get(&id)) {
                        view.open_devtools();
                    }
                }
                WindowEvent::KeyboardInput { event, .. }
                    if event.state == ElementState::Pressed
                        && primary_modifier_pressed(modifiers) =>
                {
                    match event.physical_key {
                        KeyCode::KeyL => {
                            focus_location(&window, &chrome, &mut palette_open);
                        }
                        KeyCode::KeyT
                            if add_tab(
                                &window,
                                &mut tabs,
                                &mut views,
                                &mut histories,
                                &content_events_tx,
                                &commands_tx,
                                &current_theme,
                                NEW_TAB_URL,
                                sidebar_visible,
                                settings.search_engine,
                            )
                            .is_ok() =>
                        {
                            send_state(
                                &chrome,
                                &tabs,
                                &bookmarks,
                                &settings,
                                mcp_enabled,
                                &mcp_http_state,
                            );
                            bring_chrome_to_front(&chrome);
                            focus_location(&window, &chrome, &mut palette_open);
                        }
                        KeyCode::KeyR => {
                            if !current_tab_is_new(&tabs)
                                && let Some(view) =
                                    tabs.current_id().and_then(|id| views.get_mut(&id))
                            {
                                let _ = view.reload();
                            }
                        }
                        KeyCode::BracketLeft | KeyCode::ArrowLeft
                            if !modifiers.alt_key() && !modifiers.shift_key() =>
                        {
                            if let Some(id) = tabs.current_id()
                                && let Some(history) = histories.get_mut(&id)
                                && history.go_back()
                            {
                                if let Some(view) = views.get_mut(&id) {
                                    let _ = view.go_back();
                                }
                                update_history_flags(&mut tabs, &histories, id);
                            }
                        }
                        KeyCode::BracketRight | KeyCode::ArrowRight
                            if !modifiers.alt_key() && !modifiers.shift_key() =>
                        {
                            if let Some(id) = tabs.current_id()
                                && let Some(history) = histories.get_mut(&id)
                                && history.go_forward()
                            {
                                if let Some(view) = views.get_mut(&id) {
                                    let _ = view.go_forward();
                                }
                                update_history_flags(&mut tabs, &histories, id);
                            }
                        }
                        KeyCode::KeyS
                            if !modifiers.alt_key()
                                && !modifiers.shift_key()
                                && if cfg!(target_os = "macos") {
                                    !modifiers.control_key()
                                } else {
                                    !modifiers.super_key()
                                } =>
                        {
                            sidebar_visible = !sidebar_visible;
                            apply_layout(&window, &chrome, &views, sidebar_visible, palette_open);
                            if sidebar_visible || palette_open {
                                bring_chrome_to_front(&chrome);
                            } else if let Some(view) =
                                tabs.current_id().and_then(|id| views.get(&id))
                            {
                                let _ = view.focus();
                            }
                        }
                        KeyCode::KeyW => {
                            if let Some(id) = tabs.current_id() {
                                let result = close_tab(
                                    &window,
                                    &mut tabs,
                                    &mut views,
                                    &mut histories,
                                    &content_events_tx,
                                    &commands_tx,
                                    &current_theme,
                                    id,
                                    sidebar_visible,
                                    settings.search_engine,
                                );
                                if result == CloseTabResult::CreatedReplacement {
                                    bring_chrome_to_front(&chrome);
                                }
                                send_state(
                                    &chrome,
                                    &tabs,
                                    &bookmarks,
                                    &settings,
                                    mcp_enabled,
                                    &mcp_http_state,
                                );
                            }
                        }
                        KeyCode::KeyI if modifiers.alt_key() => {
                            if let Some(view) = tabs.current_id().and_then(|id| views.get(&id)) {
                                view.open_devtools();
                            }
                        }
                        _ => {}
                    }
                }
                WindowEvent::CloseRequested => *control_flow = ControlFlow::Exit,
                _ => {}
            },
            Event::LoopDestroyed => {
                if let Some(handle) = mcp_http.take() {
                    handle.shutdown();
                }
            }
            _ => {}
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{
        NEW_TAB_URL, TabHistory, eval_result, internal_page_response, is_only_new_tab,
        normalize_url, parse_startup_args,
    };
    use browser_core::{SearchEngine, TabManager, Theme};
    use wry::http::{Request, StatusCode, header::CONTENT_TYPE};

    fn request_internal_page(url: &str, theme: Theme) -> wry::http::Response<Vec<u8>> {
        internal_page_response(Request::builder().uri(url).body(Vec::new()).unwrap(), theme)
    }

    #[test]
    fn normalizes_urls_and_search_queries() {
        assert_eq!(normalize_url("", SearchEngine::Google), NEW_TAB_URL);
        assert_eq!(
            normalize_url("example.com/path", SearchEngine::Google),
            "https://example.com/path"
        );
        assert_eq!(
            normalize_url("https://example.com", SearchEngine::Google),
            "https://example.com"
        );
        assert_eq!(
            normalize_url("rust wry", SearchEngine::Google),
            "https://www.google.com/search?q=rust+wry"
        );
        assert_eq!(
            normalize_url("?rust wry", SearchEngine::Google),
            "https://www.google.com/search?q=rust+wry"
        );
        assert_eq!(
            normalize_url("?rust & wry", SearchEngine::Google),
            "https://www.google.com/search?q=rust+%26+wry"
        );
        assert_eq!(
            normalize_url("https://", SearchEngine::Google),
            "https://www.google.com/search?q=https%3A%2F%2F"
        );
        assert_eq!(
            normalize_url("localhost", SearchEngine::Google),
            "https://localhost"
        );
        assert_eq!(
            normalize_url("localhost:3000", SearchEngine::Google),
            "https://localhost:3000"
        );
        assert_eq!(
            normalize_url("127.0.0.1:8080", SearchEngine::Google),
            "https://127.0.0.1:8080"
        );
    }

    #[test]
    fn uses_the_selected_search_engine() {
        assert_eq!(
            normalize_url("rust wry", SearchEngine::DuckDuckGo),
            "https://duckduckgo.com/?q=rust+wry"
        );
        assert_eq!(
            normalize_url("rust wry", SearchEngine::Bing),
            "https://www.bing.com/search?q=rust+wry"
        );
    }

    #[test]
    fn mcp_flag_does_not_replace_initial_url() {
        let (enabled, url) =
            parse_startup_args(["--mcp".to_owned(), "https://example.org".to_owned()]);
        assert!(enabled);
        assert_eq!(url, "https://example.org");
    }

    #[test]
    fn serves_all_internal_pages_through_the_rab_protocol() {
        let response = request_internal_page(NEW_TAB_URL, Theme::Dark);
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[CONTENT_TYPE], "text/html; charset=utf-8");
        assert!(
            String::from_utf8(response.into_body())
                .unwrap()
                .contains("新しいタブ")
        );
    }

    #[test]
    fn serves_internal_pages_with_the_selected_theme() {
        let themes = [
            (Theme::Dark, "color-scheme:dark", "#171816", "#a2a59d"),
            (Theme::Light, "color-scheme:light", "#f3f2eb", "#60645a"),
        ];

        for (theme, color_scheme, background, muted_text) in themes {
            let response = request_internal_page(NEW_TAB_URL, theme);
            let html = String::from_utf8(response.into_body()).unwrap();

            assert!(html.contains(color_scheme));
            assert!(html.contains(background));
            assert!(html.contains(muted_text));
        }
    }

    #[test]
    fn rejects_unknown_internal_pages() {
        let response = request_internal_page("rab://unknown/", Theme::Dark);

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn first_navigation_replaces_new_tab_history() {
        let mut history = TabHistory::new(NEW_TAB_URL.to_owned());

        history.record_page_load("https://example.com".to_owned());

        assert_eq!(history.entries, ["https://example.com"]);
        assert!(!history.can_go_back());
    }

    #[test]
    fn later_navigations_are_added_to_history() {
        let mut history = TabHistory::new(NEW_TAB_URL.to_owned());
        history.record_page_load("https://example.com".to_owned());

        history.record_page_load("https://example.org".to_owned());

        assert_eq!(
            history.entries,
            ["https://example.com", "https://example.org"]
        );
        assert!(history.can_go_back());
    }

    #[test]
    fn identifies_the_only_new_tab() {
        let mut tabs = TabManager::new();
        let new_tab = tabs.add_tab(NEW_TAB_URL);
        assert!(is_only_new_tab(&tabs, new_tab));

        tabs.add_tab("https://example.com");
        assert!(!is_only_new_tab(&tabs, new_tab));
    }

    #[test]
    fn decodes_webview_evaluation_results() {
        assert_eq!(
            eval_result(r#"{"ok":true,"value":"hello"}"#.to_owned()),
            Ok("hello".to_owned())
        );
        assert_eq!(
            eval_result(r#"{"ok":true,"value":"ERR:element not found"}"#.to_owned()),
            Ok("ERR:element not found".to_owned())
        );
        assert_eq!(
            eval_result(r#"{"ok":true,"value":42}"#.to_owned()),
            Ok("42".to_owned())
        );
        assert_eq!(
            eval_result(r#"{"ok":false,"error":"element not found"}"#.to_owned()),
            Err("element not found".to_owned())
        );
    }
}
