//! wry-backed implementation of the browser engine.

use browser_core::{BrowserEngine, BrowserError};
use tao::window::Window;
use wry::{PageLoadEvent, Rect, WebView, WebViewBuilder, http::Request};

const KEYBOARD_SHORTCUT_SCRIPT: &str = r#"
(() => {
  if (window.__rabKeyboardShortcutsInstalled) return;
  window.__rabKeyboardShortcutsInstalled = true;

  document.addEventListener("keydown", (event) => {
    if (!event.metaKey || event.repeat) return;

    const key = event.key.toLowerCase();
    const commandOnly = !event.altKey && !event.ctrlKey && !event.shiftKey;
    let type = null;
    if (commandOnly && key === "t") type = "new_tab";
    else if (commandOnly && key === "l") type = "open_location";
    else if (commandOnly && key === "r") type = "reload";
    else if (event.altKey && !event.ctrlKey && !event.shiftKey && key === "i") {
      type = "open_devtools";
    }
    if (!type) return;

    event.preventDefault();
    event.stopPropagation();
    window.ipc.postMessage(JSON.stringify({ type }));
  }, true);
})();
"#;

pub struct WryEngine {
    webview: WebView,
}

impl WryEngine {
    pub fn new(window: &Window, url: &str) -> Result<Self, wry::Error> {
        Self::new_with_handlers(window, url, |_| {}, |_, _| {}, |_| {})
    }

    pub fn new_with_handlers(
        window: &Window,
        url: &str,
        on_title_changed: impl Fn(String) + 'static,
        on_page_load: impl Fn(PageLoadEvent, String) + 'static,
        on_ipc: impl Fn(Request<String>) + 'static,
    ) -> Result<Self, wry::Error> {
        Self::new_with_handlers_and_bounds(
            window,
            url,
            None,
            on_title_changed,
            on_page_load,
            on_ipc,
        )
    }

    pub fn new_with_handlers_and_bounds(
        window: &Window,
        url: &str,
        bounds: Option<Rect>,
        on_title_changed: impl Fn(String) + 'static,
        on_page_load: impl Fn(PageLoadEvent, String) + 'static,
        on_ipc: impl Fn(Request<String>) + 'static,
    ) -> Result<Self, wry::Error> {
        let mut builder = WebViewBuilder::new()
            .with_initialization_script(KEYBOARD_SHORTCUT_SCRIPT)
            .with_url(url)
            .with_devtools(true)
            .with_document_title_changed_handler(on_title_changed)
            .with_on_page_load_handler(on_page_load)
            .with_ipc_handler(on_ipc);
        if let Some(bounds) = bounds {
            builder = builder.with_bounds(bounds);
        }
        let webview = builder.build_as_child(window)?;
        Ok(Self { webview })
    }

    pub fn set_bounds(&self, bounds: Rect) -> Result<(), wry::Error> {
        self.webview.set_bounds(bounds)
    }

    pub fn focus(&self) -> Result<(), wry::Error> {
        self.webview.focus()
    }

    pub fn set_visible(&self, visible: bool) -> Result<(), wry::Error> {
        self.webview.set_visible(visible)
    }

    /// Opens the WebView's inspector (Web Inspector on macOS, DevTools on Windows/Linux).
    pub fn open_devtools(&self) {
        self.webview.open_devtools();
    }

    pub fn webview(&self) -> &WebView {
        &self.webview
    }
}

impl BrowserEngine for WryEngine {
    fn navigate(&mut self, url: &str) -> Result<(), BrowserError> {
        self.webview
            .load_url(url)
            .map_err(|error| BrowserError::new(error.to_string()))
    }

    fn go_back(&mut self) -> Result<(), BrowserError> {
        self.webview
            .evaluate_script("history.back()")
            .map_err(|error| BrowserError::new(error.to_string()))
    }

    fn go_forward(&mut self) -> Result<(), BrowserError> {
        self.webview
            .evaluate_script("history.forward()")
            .map_err(|error| BrowserError::new(error.to_string()))
    }

    fn reload(&mut self) -> Result<(), BrowserError> {
        self.webview
            .reload()
            .map_err(|error| BrowserError::new(error.to_string()))
    }
}
