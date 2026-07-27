use tao::{
    dpi::{LogicalPosition, LogicalSize},
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};
use wry::{Rect, WebViewBuilder};

const CHROME_HEIGHT: f64 = 72.0;

fn bounds(window: &tao::window::Window, y: f64, height: f64) -> Rect {
    let scale = window.scale_factor();
    let size = window.inner_size().to_logical::<f64>(scale);
    Rect {
        position: LogicalPosition::new(0.0, y).into(),
        size: LogicalSize::new(size.width, height).into(),
    }
}

fn main() -> wry::Result<()> {
    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("rab-browser Phase 0: WebView composition")
        .with_inner_size(LogicalSize::new(1024.0, 700.0))
        .build(&event_loop)
        .expect("failed to create tao window");

    // The content view fills the window. The second view is deliberately created
    // afterwards and overlaps it at the top, so native child-view z-order is
    // observable instead of merely testing two non-overlapping rectangles.
    let content = WebViewBuilder::new()
        .with_url("https://example.com")
        .build_as_child(&window)?;

    let chrome_html = r#"
      <html><body style="margin:0;background:#202124;color:#fff;font:16px sans-serif">
        <div style="height:72px;display:flex;align-items:center;padding:0 24px;box-sizing:border-box">
          rab-browser <span style="margin-left:20px;color:#9aa0a6">chrome WebView / overlay</span>
        </div>
      </body></html>
    "#;
    let chrome = WebViewBuilder::new()
        .with_html(chrome_html)
        .build_as_child(&window)?;
    chrome.set_bounds(bounds(&window, 0.0, CHROME_HEIGHT))?;
    content.set_bounds(bounds(&window, 0.0, window.inner_size().height as f64))?;

    // Crude lifecycle smoke test requested by Phase 0: create and drop temporary
    // child WebViews several times while the main content view is alive. This is
    // not a memory profile; it only checks for crashes or obvious teardown issues.
    for iteration in 1..=3 {
        let temporary = WebViewBuilder::new()
            .with_html(format!(
                "<html><body>temporary view {iteration}</body></html>"
            ))
            .build_as_child(&window)?;
        temporary.set_bounds(bounds(&window, CHROME_HEIGHT, 1.0))?;
        drop(temporary);
    }

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        if let Event::WindowEvent { event, .. } = event {
            match event {
                WindowEvent::Resized(_) => {
                    // Both native child views follow the window. The chrome is
                    // created last, so it remains the frontmost overlapping view.
                    let _ =
                        content.set_bounds(bounds(&window, 0.0, window.inner_size().height as f64));
                    let _ = chrome.set_bounds(bounds(&window, 0.0, CHROME_HEIGHT));
                }
                WindowEvent::CloseRequested => *control_flow = ControlFlow::Exit,
                _ => {}
            }
        }
    });
}
