use std::{env, sync::mpsc};

use browser_core::{BrowserEngine, TabManager};
use browser_engine_wry::WryEngine;
use tao::{
    dpi::{LogicalPosition, LogicalSize},
    event::{ElementState, Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    keyboard::{KeyCode, ModifiersState},
    window::WindowBuilder,
};
use wry::{Rect, WebView, WebViewBuilder, http::Request};

const CHROME_HEIGHT: f64 = 56.0;
const DEFAULT_URL: &str = "https://example.com";

fn bounds(window: &tao::window::Window, y: f64, height: f64) -> Rect {
    let scale = window.scale_factor();
    let size = window.inner_size().to_logical::<f64>(scale);
    Rect {
        position: LogicalPosition::new(0.0, y).into(),
        size: LogicalSize::new(size.width, height).into(),
    }
}

/// Window inner height in logical (CSS-pixel-equivalent) units. `Window::inner_size()`
/// returns physical pixels, so callers computing a content height (e.g. `full - CHROME_HEIGHT`)
/// must go through this instead of `inner_size().height as f64` or the result is ~scale_factor
/// times too tall on HiDPI displays, inflating page-side `100dvh`/`vh` layouts.
fn logical_window_height(window: &tao::window::Window) -> f64 {
    window.inner_size().to_logical::<f64>(window.scale_factor()).height
}

fn chrome_html(url: &str) -> String {
    format!(
        r#"<!doctype html><html><body style="margin:0;background:#202124;color:#fff;font:14px -apple-system,sans-serif">
<form id="bar" style="height:{CHROME_HEIGHT}px;display:flex;align-items:center;gap:10px;padding:0 16px;box-sizing:border-box">
<span style="font-weight:600">rab-browser</span><input id="url" value="{url}" autocomplete="off" style="flex:1;height:30px;border:0;border-radius:5px;padding:0 10px;font:inherit;box-sizing:border-box">
</form><script>
const urlInput=document.getElementById('url');
document.getElementById('bar').addEventListener('submit',e=>{{e.preventDefault();window.ipc.postMessage('navigate:'+encodeURIComponent(urlInput.value));}});
document.addEventListener('keydown',e=>{{if(!e.metaKey)return;const key=e.key.toLowerCase();if(key==='l'){{e.preventDefault();urlInput.focus();urlInput.select();}}if(key==='r'){{e.preventDefault();window.ipc.postMessage('reload');}}}});
</script>
</body></html>"#
    )
}

fn focus_url(chrome: &WebView) {
    let _ = chrome.evaluate_script(
        "document.getElementById('url').focus(); document.getElementById('url').select();",
    );
}

fn main() -> wry::Result<()> {
    let initial_url = env::args().nth(1).unwrap_or_else(|| DEFAULT_URL.to_owned());
    let mut tabs = TabManager::new();
    tabs.add_tab(initial_url.clone());

    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("rab-browser")
        .with_inner_size(LogicalSize::new(1100.0, 760.0))
        .build(&event_loop)
        .expect("failed to create tao window");

    let mut content = WryEngine::new(&window, &initial_url)?;
    content.set_bounds(bounds(
        &window,
        CHROME_HEIGHT,
        logical_window_height(&window) - CHROME_HEIGHT,
    ))?;

    let (commands_tx, commands_rx) = mpsc::channel::<String>();
    let chrome = WebViewBuilder::new()
        .with_html(chrome_html(&initial_url))
        .with_ipc_handler(move |request: Request<String>| {
            let _ = commands_tx.send(request.into_body());
        })
        .build_as_child(&window)?;
    chrome.set_bounds(bounds(&window, 0.0, CHROME_HEIGHT))?;

    let mut modifiers = ModifiersState::empty();
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::MainEventsCleared => {
                for command in commands_rx.try_iter() {
                    if let Some(encoded_url) = command.strip_prefix("navigate:") {
                        if let Ok(url) = percent_decode(encoded_url) {
                            if content.navigate(&url).is_ok() {
                                if let Some(tab) = tabs.current_tab_mut() {
                                    tab.url = url;
                                }
                            }
                        }
                    }
                    if command == "reload" {
                        let _ = content.reload();
                    }
                }
            }
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::Resized(_) => {
                    let _ = content.set_bounds(bounds(
                        &window,
                        CHROME_HEIGHT,
                        logical_window_height(&window) - CHROME_HEIGHT,
                    ));
                    let _ = chrome.set_bounds(bounds(&window, 0.0, CHROME_HEIGHT));
                }
                WindowEvent::ModifiersChanged(state) => modifiers = state,
                WindowEvent::KeyboardInput { event, .. }
                    if event.state == ElementState::Pressed && modifiers.super_key() =>
                {
                    match event.physical_key {
                        KeyCode::KeyL => focus_url(&chrome),
                        KeyCode::KeyR => {
                            let _ = content.reload();
                        }
                        KeyCode::KeyI if modifiers.alt_key() => {
                            content.open_devtools();
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

fn percent_decode(value: &str) -> Result<String, ()> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err(());
            }
            let high = hex(bytes[index + 1]).ok_or(())?;
            let low = hex(bytes[index + 2]).ok_or(())?;
            decoded.push(high * 16 + low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).map_err(|_| ())
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
