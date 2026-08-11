//! wry-backed implementation of the browser engine.

use std::borrow::Cow;

use browser_core::{BrowserEngine, BrowserError};
use tao::window::Window;
use wry::{
    PageLoadEvent, Rect, WebView, WebViewBuilder,
    http::{Request, Response},
};

#[cfg(target_os = "macos")]
use objc2::{MainThreadMarker, rc::Retained};
#[cfg(target_os = "macos")]
use objc2_app_kit::{NSAutoresizingMaskOptions, NSView};
#[cfg(target_os = "macos")]
use objc2_foundation::{NSPoint, NSRect, NSSize};
#[cfg(target_os = "macos")]
use wry::WebViewExtMacOS;

#[cfg(target_os = "macos")]
mod macos_js_dialogs;

#[cfg(target_os = "macos")]
pub use macos_js_dialogs::{RabUIDelegate, install_js_dialog_delegate};

#[cfg(not(target_os = "macos"))]
pub fn install_js_dialog_delegate(_: &wry::WebView) {}

/// WKWebView's default UA string doesn't match a released Safari version, so
/// some sites (e.g. Google Search) serve stale/legacy markup. Present as a
/// current Safari on macOS instead.
const MODERN_USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15";

const KEYBOARD_SHORTCUT_SCRIPT: &str = r#"
(() => {
  if (window.__rabContentIntegrationInstalled) return;
  window.__rabContentIntegrationInstalled = true;

  const isMac = /Mac|iPhone|iPad|iPod/.test(navigator.platform);
  const hasPrimaryModifier = (event) => isMac ? event.metaKey : event.ctrlKey;
  const hasSecondaryPrimaryModifier = (event) =>
    isMac ? event.ctrlKey : event.metaKey;
  const NEW_TAB_URL = "rab://newtab/";
  const isNewTabUrl = (url) => url === "about:blank" || url === NEW_TAB_URL;
  const postMessage = (message) => {
    window.ipc.postMessage(JSON.stringify(message));
  };

  document.addEventListener("keydown", (event) => {
    if (event.repeat) return;

    const key = event.key.toLowerCase();
    if (key === "f12") {
      event.preventDefault();
      event.stopPropagation();
      postMessage({ type: "open_devtools" });
      return;
    }
    if (!hasPrimaryModifier(event)) return;

    const primaryOnly =
      !event.altKey && !hasSecondaryPrimaryModifier(event) && !event.shiftKey;
    let type = null;
    if (primaryOnly && key === "t") type = "new_tab";
    else if (primaryOnly && key === "l") type = "open_location";
    else if (primaryOnly && key === "r" && !isNewTabUrl(location.href)) {
      type = "reload";
    }
    else if (primaryOnly && key === "s") type = "toggle_sidebar";
    else if (primaryOnly && key === "w") type = "close_current_tab";
    else if (primaryOnly && (key === "[" || key === "arrowleft")) type = "go_back";
    else if (primaryOnly && (key === "]" || key === "arrowright")) type = "go_forward";
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

  let lastFaviconUrl;
  const notifyFaviconChanged = () => {
    const icon = document.querySelector(
      'link[rel~="icon"], link[rel="shortcut icon"]',
    );
    const url = icon?.href ?? "";
    if (url === lastFaviconUrl) return;
    lastFaviconUrl = url;
    postMessage({ type: "favicon_changed", url });
  };

  document.addEventListener("DOMContentLoaded", notifyFaviconChanged);
  window.addEventListener("load", notifyFaviconChanged);
  // This script runs at document-start, before <head> necessarily exists, so
  // the observer can't just be attached once here (document.head may still
  // be null). Attach to <head> as soon as it's available: immediately if
  // it's already there, otherwise via a one-shot observer on <html> that
  // watches for <head> to be inserted and then re-targets itself.
  const observeHeadFavicon = (head) => {
    new MutationObserver(notifyFaviconChanged).observe(head, {
      childList: true,
      subtree: true,
      attributes: true,
      attributeFilter: ["href", "rel"],
    });
  };
  if (document.head) {
    observeHeadFavicon(document.head);
  } else {
    const rootObserver = new MutationObserver(() => {
      if (!document.head) return;
      rootObserver.disconnect();
      observeHeadFavicon(document.head);
    });
    rootObserver.observe(document.documentElement, { childList: true });
  }

  let lastMediaPlaying;
  const notifyMediaPlaybackChanged = () => {
    const playing = [...document.querySelectorAll("video, audio")].some(
      (media) => !media.paused && !media.ended,
    );
    if (playing === lastMediaPlaying) return;
    lastMediaPlaying = playing;
    postMessage({ type: "media_playback_changed", playing });
  };

  document.addEventListener("play", notifyMediaPlaybackChanged, true);
  document.addEventListener("pause", notifyMediaPlaybackChanged, true);
  document.addEventListener("ended", notifyMediaPlaybackChanged, true);
  document.addEventListener("DOMContentLoaded", notifyMediaPlaybackChanged);
})();
"#;

pub struct WryEngine {
    // Fields drop in declaration order, so the webview releases its weak
    // UIDelegate reference before the retained delegate is dropped.
    webview: WebView,
    _ui_delegate: InstallJsDialogDelegateResult,
    #[cfg(target_os = "macos")]
    container: Retained<NSView>,
}

impl Drop for WryEngine {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        self.container.removeFromSuperview();
    }
}

#[cfg(target_os = "macos")]
type InstallJsDialogDelegateResult = Option<objc2::rc::Retained<RabUIDelegate>>;

#[cfg(not(target_os = "macos"))]
type InstallJsDialogDelegateResult = ();

type CustomProtocolHandler = Box<dyn Fn(Request<Vec<u8>>) -> Response<Vec<u8>>>;

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
        Self::build(
            window,
            url,
            bounds,
            on_title_changed,
            on_page_load,
            on_ipc,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_with_handlers_and_bounds_and_protocol(
        window: &Window,
        url: &str,
        bounds: Option<Rect>,
        on_title_changed: impl Fn(String) + 'static,
        on_page_load: impl Fn(PageLoadEvent, String) + 'static,
        on_ipc: impl Fn(Request<String>) + 'static,
        protocol_name: &str,
        protocol_handler: impl Fn(Request<Vec<u8>>) -> Response<Vec<u8>> + 'static,
    ) -> Result<Self, wry::Error> {
        Self::build(
            window,
            url,
            bounds,
            on_title_changed,
            on_page_load,
            on_ipc,
            Some((protocol_name.to_owned(), Box::new(protocol_handler))),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn build(
        window: &Window,
        url: &str,
        bounds: Option<Rect>,
        on_title_changed: impl Fn(String) + 'static,
        on_page_load: impl Fn(PageLoadEvent, String) + 'static,
        on_ipc: impl Fn(Request<String>) + 'static,
        custom_protocol: Option<(String, CustomProtocolHandler)>,
    ) -> Result<Self, wry::Error> {
        let mut builder = WebViewBuilder::new()
            .with_initialization_script(KEYBOARD_SHORTCUT_SCRIPT)
            .with_url(url)
            .with_user_agent(MODERN_USER_AGENT)
            .with_devtools(true)
            .with_back_forward_navigation_gestures(true)
            .with_document_title_changed_handler(on_title_changed)
            .with_on_page_load_handler(on_page_load)
            .with_ipc_handler(on_ipc);
        if let Some((name, handler)) = custom_protocol {
            builder = builder.with_custom_protocol(name, move |_webview_id, request| {
                handler(request).map(Cow::Owned)
            });
        }
        if let Some(bounds) = bounds {
            builder = builder.with_bounds(bounds);
        }
        let webview = builder.build_as_child(window)?;
        #[cfg(target_os = "macos")]
        let container = attach_to_offset_container(window, &webview, bounds);
        let ui_delegate = install_js_dialog_delegate(&webview);
        Ok(Self {
            webview,
            _ui_delegate: ui_delegate,
            #[cfg(target_os = "macos")]
            container,
        })
    }

    /// Repositions the content WebView. On macOS this moves the offset
    /// container (see `attach_to_offset_container`) rather than the WKWebView
    /// itself, so the Web Inspector keeps docking to its right instead of
    /// under the sidebar. Do not add a variant that sets the WKWebView's own
    /// frame directly on macOS: that would desync it from the container's
    /// bounds, which the inspector uses to size itself.
    pub fn set_content_bounds(&self, window: &Window, bounds: Rect) -> Result<(), wry::Error> {
        #[cfg(target_os = "macos")]
        {
            self.container.setFrame(appkit_rect(window, bounds));
            Ok(())
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = window;
            self.webview.set_bounds(bounds)
        }
    }

    pub fn focus(&self) -> Result<(), wry::Error> {
        self.webview.focus()
    }

    pub fn set_visible(&self, visible: bool) -> Result<(), wry::Error> {
        #[cfg(target_os = "macos")]
        {
            self.container.setHidden(!visible);
            Ok(())
        }
        #[cfg(not(target_os = "macos"))]
        {
            self.webview.set_visible(visible)
        }
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

#[cfg(target_os = "macos")]
fn attach_to_offset_container(
    window: &Window,
    webview: &WebView,
    bounds: Option<Rect>,
) -> Retained<NSView> {
    let mtm = MainThreadMarker::new().expect("WKWebView creation must run on the main thread");
    let container = NSView::new(mtm);
    let bounds = bounds.unwrap_or(Rect {
        position: tao::dpi::LogicalPosition::new(0.0, 0.0).into(),
        size: window
            .inner_size()
            .to_logical::<f64>(window.scale_factor())
            .into(),
    });
    container.setFrame(appkit_rect(window, bounds));
    container.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
    );

    let native_webview = webview.webview();
    let native_window = native_webview
        .window()
        .expect("a child WKWebView must be attached to an NSWindow");
    let content_view = native_window
        .contentView()
        .expect("an NSWindow containing a WKWebView must have a content view");

    // Web Inspector sizes itself from inspectedView.superview.bounds. Making
    // the sidebar-offset container that superview keeps the inspector to the
    // right of the sidebar. Content views use the full window height here, so
    // the AppKit Y-axis flip can be ignored.
    native_webview.removeFromSuperview();
    content_view.addSubview(&container);
    container.addSubview(&native_webview);
    native_webview.setFrame(container.bounds());
    native_webview.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
    );

    container
}

#[cfg(target_os = "macos")]
/// Converts a `Rect` in tao's top-left-origin logical coordinates to an
/// `NSRect` in the window's content view coordinate space, which AppKit
/// anchors at the bottom-left unless the view opts into a flipped
/// coordinate system (our container/content view don't).
fn appkit_rect(window: &Window, bounds: Rect) -> NSRect {
    let scale_factor = window.scale_factor();
    let position = bounds.position.to_logical::<f64>(scale_factor);
    let size = bounds.size.to_logical::<f64>(scale_factor);
    let window_height = window
        .inner_size()
        .to_logical::<f64>(scale_factor)
        .height;
    NSRect::new(
        NSPoint::new(position.x, window_height - position.y - size.height),
        NSSize::new(size.width, size.height),
    )
}

impl BrowserEngine for WryEngine {
    fn navigate(&mut self, url: &str) -> Result<(), BrowserError> {
        self.webview
            .load_url(url)
            .map_err(|error| BrowserError::new(error.to_string()))
    }

    fn navigate_replacing(&mut self, url: &str) -> Result<(), BrowserError> {
        // `location.replace` performs the navigation without pushing a new
        // entry onto WKWebView's native back/forward list, unlike `load_url`.
        let encoded_url =
            serde_json::to_string(url).map_err(|error| BrowserError::new(error.to_string()))?;
        self.webview
            .evaluate_script(&format!("location.replace({encoded_url})"))
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
