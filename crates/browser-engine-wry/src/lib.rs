//! wry-backed implementation of the browser engine.

use browser_core::{BrowserEngine, BrowserError};
use tao::window::Window;
use wry::{Rect, WebView, WebViewBuilder};

pub struct WryEngine {
    webview: WebView,
}

impl WryEngine {
    pub fn new(window: &Window, url: &str) -> Result<Self, wry::Error> {
        let webview = WebViewBuilder::new().with_url(url).build_as_child(window)?;
        Ok(Self { webview })
    }

    pub fn set_bounds(&self, bounds: Rect) -> Result<(), wry::Error> {
        self.webview.set_bounds(bounds)
    }

    pub fn focus(&self) -> Result<(), wry::Error> {
        self.webview.focus()
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
