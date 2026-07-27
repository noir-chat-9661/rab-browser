//! wry-backed implementation of the browser engine.

use browser_core::{BrowserEngine, BrowserError};
use tao::window::Window;
use wry::{PageLoadEvent, Rect, WebView, WebViewBuilder, http::Request};

const KEYBOARD_SHORTCUT_SCRIPT: &str = r#"
(() => {
  if (window.__rabContentIntegrationInstalled) return;
  window.__rabContentIntegrationInstalled = true;

  const isMac = /Mac|iPhone|iPad|iPod/.test(navigator.platform);
  const hasPrimaryModifier = (event) => isMac ? event.metaKey : event.ctrlKey;
  const hasSecondaryPrimaryModifier = (event) =>
    isMac ? event.ctrlKey : event.metaKey;
  const postMessage = (message) => {
    window.ipc.postMessage(JSON.stringify(message));
  };

  document.addEventListener("keydown", (event) => {
    if (!hasPrimaryModifier(event) || event.repeat) return;

    const key = event.key.toLowerCase();
    const primaryOnly =
      !event.altKey && !hasSecondaryPrimaryModifier(event) && !event.shiftKey;
    let type = null;
    if (primaryOnly && key === "t") type = "new_tab";
    else if (primaryOnly && key === "l") type = "open_location";
    else if (primaryOnly && key === "r") type = "reload";
    else if (primaryOnly && key === "w") type = "close_current_tab";
    else if (
      event.altKey &&
      !hasSecondaryPrimaryModifier(event) &&
      !event.shiftKey &&
      key === "i"
    ) {
      type = "open_devtools";
    }
    if (!type) return;

    event.preventDefault();
    event.stopPropagation();
    postMessage({ type });
  }, true);

  document.addEventListener("click", (event) => {
    if (!hasPrimaryModifier(event)) return;

    const target =
      event.target instanceof Element ? event.target : event.target?.parentElement;
    const link = target?.closest("a[href]");
    if (!link) return;

    event.preventDefault();
    postMessage({ type: "new_tab", url: link.href });
  }, true);

  let lastUrl = location.href;
  const notifyUrlChanged = () => {
    if (location.href === lastUrl) return;
    lastUrl = location.href;
    postMessage({ type: "content_url_changed", url: location.href });
  };

  for (const method of ["pushState", "replaceState"]) {
    const original = history[method];
    history[method] = function (...args) {
      const result = original.apply(this, args);
      notifyUrlChanged();
      return result;
    };
  }
  window.addEventListener("popstate", notifyUrlChanged);
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

    pub fn evaluate_script_with_callback(
        &self,
        script: &str,
        callback: impl Fn(String) + Send + 'static,
    ) -> Result<(), wry::Error> {
        self.webview.evaluate_script_with_callback(script, callback)
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
