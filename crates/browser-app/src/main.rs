use std::{
    collections::BTreeMap,
    env, fs,
    sync::mpsc::{self, Sender},
};

use browser_core::{BrowserEngine, TabId, TabManager};
use browser_engine_wry::WryEngine;
#[cfg(target_os = "macos")]
use objc2::{rc::Retained, runtime::AnyObject};
#[cfg(target_os = "macos")]
use objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags};
use serde::{Deserialize, Serialize};
use tao::{
    dpi::{LogicalPosition, LogicalSize},
    event::{ElementState, Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    keyboard::{KeyCode, ModifiersState},
    window::{Window, WindowBuilder},
};
use wry::{PageLoadEvent, Rect, WebView, WebViewBuilder, http::Request};

const SIDEBAR_WIDTH: f64 = 264.0;
const DEFAULT_URL: &str = "https://example.com";
const NEW_TAB_URL: &str = concat!(
    "data:text/html;charset=utf-8,",
    "%3C!doctype%20html%3E%3Chtml%20lang=%22ja%22%3E%3Chead%3E",
    "%3Cmeta%20charset=%22utf-8%22%3E%3Ctitle%3E%E6%96%B0%E3%81%97%E3%81%84%E3%82%BF%E3%83%96%3C/title%3E",
    "%3Cstyle%3Ehtml%2Cbody%7Bheight%3A100%25%7Dbody%7Bmargin%3A0%3Bdisplay%3Agrid%3Bplace-items%3Acenter%3B",
    "background%3A%23171816%3Bcolor%3A%23a2a59d%3Bfont%3A14px%20system-ui%2Csans-serif%7D%3C/style%3E",
    "%3C/head%3E%3Cbody%3E%E6%96%B0%E3%81%97%E3%81%84%E3%82%BF%E3%83%96%3C/body%3E%3C/html%3E"
);

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
    OpenDevtools,
    PaletteOpened,
    PaletteClosed,
}

#[derive(Debug)]
enum ContentEvent {
    TitleChanged { id: TabId, title: String },
    PageLoaded { id: TabId, url: String },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChromeState<'a> {
    r#type: &'static str,
    tabs: Vec<ChromeTab<'a>>,
    current_tab_id: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChromeTab<'a> {
    id: u64,
    url: &'a str,
    title: &'a str,
    can_go_back: bool,
    can_go_forward: bool,
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

fn content_bounds(window: &Window) -> Rect {
    let size = logical_window_size(window);
    let sidebar_width = SIDEBAR_WIDTH.min(size.width);
    Rect {
        position: LogicalPosition::new(sidebar_width, 0.0).into(),
        size: LogicalSize::new((size.width - sidebar_width).max(0.0), size.height).into(),
    }
}

fn chrome_html() -> String {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../ui-chrome/dist/index.html"
    );
    fs::read_to_string(path).unwrap_or_else(|_| {
        "<!doctype html><body style=\"margin:0;background:#171816;color:#eee;font:14px sans-serif;padding:24px\">\
         ui-chrome is not built.<br><br>Run <code>pnpm --dir ui-chrome build</code>.</body>"
            .to_owned()
    })
}

fn normalize_url(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        return NEW_TAB_URL.to_owned();
    }
    if let Some(query) = value.strip_prefix('?') {
        return search_url(query.trim());
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
        search_url(value)
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

    if host.parse::<std::net::IpAddr>().is_ok() {
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

fn search_url(query: &str) -> String {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("q", query)
        .finish();
    format!("https://www.google.com/search?{query}")
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
    event_loop_proxy: tao::event_loop::EventLoopProxy<()>,
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
                let _ = event_loop_proxy.send_event(());
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

fn resolve_tab_id(tabs: &TabManager, raw_id: u64) -> Option<TabId> {
    tabs.tabs()
        .find(|tab| tab.id.get() == raw_id)
        .map(|tab| tab.id)
}

fn create_content_view(
    window: &Window,
    id: TabId,
    url: &str,
    events_tx: &Sender<ContentEvent>,
    commands_tx: &Sender<String>,
) -> wry::Result<WryEngine> {
    let title_tx = events_tx.clone();
    let load_tx = events_tx.clone();
    let content_commands_tx = commands_tx.clone();
    let bounds = content_bounds(window);
    let view = WryEngine::new_with_handlers_and_bounds(
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
            let _ = content_commands_tx.send(request.into_body());
        },
    )?;
    // Keep this as a post-build correction too: the window scale or size may
    // have changed while WKWebView was being initialized.
    view.set_bounds(content_bounds(window))?;
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

fn select_content_view(tabs: &mut TabManager, views: &BTreeMap<TabId, WryEngine>, id: TabId) {
    if !tabs.select_tab(id) {
        return;
    }
    for (view_id, view) in views {
        let selected = *view_id == id;
        let _ = view.set_visible(selected);
        if selected {
            let _ = view.focus();
        }
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

fn send_state(chrome: &WebView, tabs: &TabManager) {
    let state = ChromeState {
        r#type: "state",
        tabs: tabs
            .tabs()
            .map(|tab| ChromeTab {
                id: tab.id.get(),
                url: &tab.url,
                title: &tab.title,
                can_go_back: tab.can_go_back,
                can_go_forward: tab.can_go_forward,
            })
            .collect(),
        current_tab_id: tabs.current_id().map(TabId::get),
    };
    if let Ok(json) = serde_json::to_string(&state) {
        let _ = chrome.evaluate_script(&format!("window.rabChrome?.receive({json});"));
    }
}

fn add_tab(
    window: &Window,
    tabs: &mut TabManager,
    views: &mut BTreeMap<TabId, WryEngine>,
    histories: &mut BTreeMap<TabId, TabHistory>,
    events_tx: &Sender<ContentEvent>,
    commands_tx: &Sender<String>,
    url: &str,
) -> wry::Result<TabId> {
    let url = normalize_url(url);
    let id = tabs.add_tab(url.clone());
    match create_content_view(window, id, &url, events_tx, commands_tx) {
        Ok(view) => {
            histories.insert(id, TabHistory::new(url));
            views.insert(id, view);
            select_content_view(tabs, views, id);
            Ok(id)
        }
        Err(error) => {
            tabs.remove_tab(id);
            Err(error)
        }
    }
}

fn close_tab(
    window: &Window,
    tabs: &mut TabManager,
    views: &mut BTreeMap<TabId, WryEngine>,
    histories: &mut BTreeMap<TabId, TabHistory>,
    events_tx: &Sender<ContentEvent>,
    commands_tx: &Sender<String>,
    id: TabId,
) -> bool {
    views.remove(&id);
    histories.remove(&id);
    tabs.remove_tab(id);

    if tabs.current_id().is_none() {
        return add_tab(
            window,
            tabs,
            views,
            histories,
            events_tx,
            commands_tx,
            NEW_TAB_URL,
        )
        .is_ok();
    } else if let Some(current) = tabs.current_id() {
        select_content_view(tabs, views, current);
    }
    false
}

fn focus_location(chrome: &WebView) {
    let _ = chrome.focus();
    let _ = chrome.evaluate_script("window.rabChrome?.openLocation();");
}

fn main() -> wry::Result<()> {
    let initial_url = env::args().nth(1).unwrap_or_else(|| DEFAULT_URL.to_owned());
    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("rab-browser")
        .with_inner_size(LogicalSize::new(1180.0, 760.0))
        .with_min_inner_size(LogicalSize::new(620.0, 420.0))
        .build(&event_loop)
        .expect("failed to create tao window");

    let (content_events_tx, content_events_rx) = mpsc::channel::<ContentEvent>();
    let (commands_tx, commands_rx) = mpsc::channel::<String>();
    #[cfg(target_os = "macos")]
    let close_tab_shortcut_monitor =
        install_close_tab_shortcut_monitor(commands_tx.clone(), event_loop.create_proxy())
            .expect("failed to install the macOS Cmd+W event monitor");
    let mut tabs = TabManager::new();
    let mut views = BTreeMap::new();
    let mut histories = BTreeMap::new();
    add_tab(
        &window,
        &mut tabs,
        &mut views,
        &mut histories,
        &content_events_tx,
        &commands_tx,
        &initial_url,
    )?;

    let chrome_commands_tx = commands_tx.clone();
    let chrome = WebViewBuilder::new()
        .with_html(chrome_html())
        .with_transparent(true)
        .with_devtools(true)
        .with_ipc_handler(move |request: Request<String>| {
            let _ = chrome_commands_tx.send(request.into_body());
        })
        .build_as_child(&window)?;
    chrome.set_bounds(chrome_bounds(&window))?;

    let mut modifiers = ModifiersState::empty();
    let mut palette_open = false;
    event_loop.run(move |event, _, control_flow| {
        #[cfg(target_os = "macos")]
        // Keep the monitor token with the event loop so its registration has the
        // same explicit lifetime as the AppKit application.
        let _keep_close_tab_shortcut_monitor_alive = &close_tab_shortcut_monitor;

        *control_flow = ControlFlow::Wait;
        match event {
            Event::MainEventsCleared => {
                for event in content_events_rx.try_iter() {
                    match event {
                        ContentEvent::TitleChanged { id, title } => {
                            if let Some(tab) = tabs.tab_mut(id) {
                                tab.title = title;
                            }
                        }
                        ContentEvent::PageLoaded { id, url } => {
                            if let Some(tab) = tabs.tab_mut(id) {
                                tab.url = url.clone();
                            }
                            if let Some(history) = histories.get_mut(&id) {
                                history.record_page_load(url);
                            }
                            update_history_flags(&mut tabs, &histories, id);
                        }
                    }
                    send_state(&chrome, &tabs);
                }

                for raw_command in commands_rx.try_iter() {
                    let Ok(command) = serde_json::from_str::<ChromeCommand>(&raw_command) else {
                        continue;
                    };
                    match command {
                        ChromeCommand::ChromeReady => {}
                        ChromeCommand::SelectTab { id } => {
                            if let Some(id) = resolve_tab_id(&tabs, id) {
                                select_content_view(&mut tabs, &views, id);
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
                                url.as_deref().unwrap_or(NEW_TAB_URL),
                            )
                            .is_ok()
                            {
                                bring_chrome_to_front(&chrome);
                                send_state(&chrome, &tabs);
                                focus_location(&chrome);
                            }
                        }
                        ChromeCommand::CloseTab { id } => {
                            if let Some(id) = resolve_tab_id(&tabs, id) {
                                let created_replacement = close_tab(
                                    &window,
                                    &mut tabs,
                                    &mut views,
                                    &mut histories,
                                    &content_events_tx,
                                    &commands_tx,
                                    id,
                                );
                                if created_replacement {
                                    bring_chrome_to_front(&chrome);
                                }
                            }
                        }
                        ChromeCommand::CloseCurrentTab => {
                            if let Some(id) = tabs.current_id() {
                                let created_replacement = close_tab(
                                    &window,
                                    &mut tabs,
                                    &mut views,
                                    &mut histories,
                                    &content_events_tx,
                                    &commands_tx,
                                    id,
                                );
                                if created_replacement {
                                    bring_chrome_to_front(&chrome);
                                }
                            }
                        }
                        ChromeCommand::Navigate { url } => {
                            if let Some(id) = tabs.current_id() {
                                let url = normalize_url(&url);
                                if let Some(view) = views.get_mut(&id)
                                    && view.navigate(&url).is_ok()
                                    && let Some(tab) = tabs.tab_mut(id)
                                {
                                    tab.url = url;
                                }
                            }
                        }
                        ChromeCommand::ContentUrlChanged { url } => {
                            if let Some(id) = tabs.current_id() {
                                if let Some(tab) = tabs.tab_mut(id) {
                                    tab.url = url.clone();
                                }
                                if let Some(history) = histories.get_mut(&id) {
                                    history.record_page_load(url);
                                }
                                update_history_flags(&mut tabs, &histories, id);
                            }
                        }
                        ChromeCommand::OpenLocation => focus_location(&chrome),
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
                            if let Some(view) = tabs.current_id().and_then(|id| views.get_mut(&id))
                            {
                                let _ = view.reload();
                            }
                        }
                        ChromeCommand::OpenDevtools => {
                            if let Some(view) = tabs.current_id().and_then(|id| views.get(&id)) {
                                view.open_devtools();
                            }
                        }
                        ChromeCommand::PaletteOpened => {
                            palette_open = true;
                            let _ = chrome.set_bounds(full_window_bounds(&window));
                            bring_chrome_to_front(&chrome);
                            let _ = chrome.focus();
                        }
                        ChromeCommand::PaletteClosed => {
                            palette_open = false;
                            let _ = chrome.set_bounds(chrome_bounds(&window));
                            if let Some(view) = tabs.current_id().and_then(|id| views.get(&id)) {
                                let _ = view.focus();
                            }
                        }
                    }
                    send_state(&chrome, &tabs);
                }
            }
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::Resized(_) => {
                    let bounds = content_bounds(&window);
                    for view in views.values() {
                        let _ = view.set_bounds(bounds);
                    }
                    let chrome_rect = if palette_open {
                        full_window_bounds(&window)
                    } else {
                        chrome_bounds(&window)
                    };
                    let _ = chrome.set_bounds(chrome_rect);
                }
                WindowEvent::ModifiersChanged(state) => modifiers = state,
                WindowEvent::KeyboardInput { event, .. }
                    if event.state == ElementState::Pressed
                        && primary_modifier_pressed(modifiers) =>
                {
                    match event.physical_key {
                        KeyCode::KeyL => focus_location(&chrome),
                        KeyCode::KeyT
                            if add_tab(
                                &window,
                                &mut tabs,
                                &mut views,
                                &mut histories,
                                &content_events_tx,
                                &commands_tx,
                                NEW_TAB_URL,
                            )
                            .is_ok() =>
                        {
                            send_state(&chrome, &tabs);
                            bring_chrome_to_front(&chrome);
                            focus_location(&chrome);
                        }
                        KeyCode::KeyR => {
                            if let Some(view) = tabs.current_id().and_then(|id| views.get_mut(&id))
                            {
                                let _ = view.reload();
                            }
                        }
                        KeyCode::KeyW => {
                            if let Some(id) = tabs.current_id() {
                                let created_replacement = close_tab(
                                    &window,
                                    &mut tabs,
                                    &mut views,
                                    &mut histories,
                                    &content_events_tx,
                                    &commands_tx,
                                    id,
                                );
                                if created_replacement {
                                    bring_chrome_to_front(&chrome);
                                }
                                send_state(&chrome, &tabs);
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
            _ => {}
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{NEW_TAB_URL, normalize_url};

    #[test]
    fn normalizes_urls_and_search_queries() {
        assert_eq!(normalize_url(""), NEW_TAB_URL);
        assert_eq!(
            normalize_url("example.com/path"),
            "https://example.com/path"
        );
        assert_eq!(normalize_url("https://example.com"), "https://example.com");
        assert_eq!(
            normalize_url("rust wry"),
            "https://www.google.com/search?q=rust+wry"
        );
        assert_eq!(
            normalize_url("?rust wry"),
            "https://www.google.com/search?q=rust+wry"
        );
        assert_eq!(
            normalize_url("?rust & wry"),
            "https://www.google.com/search?q=rust+%26+wry"
        );
        assert_eq!(
            normalize_url("https://"),
            "https://www.google.com/search?q=https%3A%2F%2F"
        );
        assert_eq!(
            normalize_url("localhost"),
            "https://www.google.com/search?q=localhost"
        );
    }
}
