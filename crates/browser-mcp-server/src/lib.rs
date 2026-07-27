//! MCP tools for controlling a running rab-browser GUI.

use std::{fmt, sync::Arc, thread::JoinHandle, time::Duration};

use rmcp::{
    ErrorData, ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ServerCapabilities, ServerInfo},
    schemars::{self, JsonSchema},
    tool, tool_handler, tool_router,
};
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

#[cfg(not(test))]
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
#[cfg(test)]
const REQUEST_TIMEOUT: Duration = Duration::from_millis(20);

/// A request sent from the MCP runtime to the browser's GUI event loop.
#[derive(Debug)]
pub enum McpRequest {
    /// Wake the GUI loop so it can drain an existing application command.
    Wake,
    ListTabs {
        reply: oneshot::Sender<Vec<TabInfo>>,
    },
    NewTab {
        url: Option<String>,
        reply: oneshot::Sender<u64>,
    },
    CloseTab {
        id: u64,
        reply: oneshot::Sender<bool>,
    },
    SelectTab {
        id: u64,
        reply: oneshot::Sender<bool>,
    },
    Navigate {
        url: String,
        reply: oneshot::Sender<Result<(), String>>,
    },
    GoBack {
        reply: oneshot::Sender<bool>,
    },
    GoForward {
        reply: oneshot::Sender<bool>,
    },
    Reload {
        reply: oneshot::Sender<Result<(), String>>,
    },
    Eval {
        target: Option<u64>,
        script: String,
        reply: oneshot::Sender<Result<String, String>>,
    },
}

/// Browser tab metadata returned by `list_tabs`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TabInfo {
    pub id: u64,
    pub url: String,
    pub title: String,
    pub active: bool,
}

/// Sends MCP requests to the browser-owned event loop.
pub trait RequestDispatcher: Send + Sync + 'static {
    fn dispatch(&self, request: McpRequest) -> Result<(), DispatchError>;
}

/// The browser event loop is no longer accepting requests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DispatchError {
    LoopClosed,
}

impl fmt::Display for DispatchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("browser event loop is closed")
    }
}

impl std::error::Error for DispatchError {}

#[derive(Debug, Deserialize, JsonSchema)]
struct UrlParams {
    /// URL or search text to open.
    url: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct NewTabParams {
    /// URL or search text to open. Omit to open the new-tab page.
    url: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct TabParams {
    /// Numeric tab ID returned by list_tabs.
    id: u64,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SelectorParams {
    /// CSS selector. Omit to use the document root.
    selector: Option<String>,
    /// Tab ID to target. Omit to use the active tab.
    target: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct EvaluateParams {
    /// JavaScript expression or statements to execute.
    script: String,
    /// Tab ID to target. Omit to use the active tab.
    target: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ClickParams {
    /// CSS selector of the element to click.
    selector: String,
    /// Tab ID to target. Omit to use the active tab.
    target: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct TypeParams {
    /// CSS selector of the input element.
    selector: String,
    /// Text to insert after replacing the current value.
    text: String,
    /// Tab ID to target. Omit to use the active tab.
    target: Option<u64>,
}

#[allow(dead_code)]
#[derive(Clone)]
struct BrowserMcpServer {
    dispatcher: Arc<dyn RequestDispatcher>,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl BrowserMcpServer {
    fn new(dispatcher: Arc<dyn RequestDispatcher>) -> Self {
        Self {
            dispatcher,
            tool_router: Self::tool_router(),
        }
    }

    async fn request<T>(
        &self,
        build_request: impl FnOnce(oneshot::Sender<T>) -> McpRequest,
    ) -> Result<T, ErrorData> {
        let (reply, receiver) = oneshot::channel();
        self.dispatcher
            .dispatch(build_request(reply))
            .map_err(|error| ErrorData::internal_error(error.to_string(), None))?;
        tokio::time::timeout(REQUEST_TIMEOUT, receiver)
            .await
            .map_err(|_| ErrorData::internal_error("browser request timed out", None))?
            .map_err(|_| ErrorData::internal_error("browser dropped the response", None))
    }

    async fn eval(&self, target: Option<u64>, script: String) -> Result<String, ErrorData> {
        self.request(|reply| McpRequest::Eval {
            target,
            script,
            reply,
        })
        .await?
        .map_err(|message| ErrorData::internal_error(message, None))
    }

    #[tool(description = "Navigate the active rab-browser tab to a URL or search query")]
    async fn navigate(&self, params: Parameters<UrlParams>) -> Result<String, ErrorData> {
        self.request(|reply| McpRequest::Navigate {
            url: params.0.url,
            reply,
        })
        .await?
        .map_err(|message| ErrorData::internal_error(message, None))?;
        Ok("ok".to_owned())
    }

    #[tool(description = "Open a new rab-browser tab and return its numeric ID")]
    async fn new_tab(&self, params: Parameters<NewTabParams>) -> Result<String, ErrorData> {
        let id = self
            .request(|reply| McpRequest::NewTab {
                url: params.0.url,
                reply,
            })
            .await?;
        Ok(id.to_string())
    }

    #[tool(description = "Close a rab-browser tab by numeric ID")]
    async fn close_tab(&self, params: Parameters<TabParams>) -> Result<String, ErrorData> {
        let closed = self
            .request(|reply| McpRequest::CloseTab {
                id: params.0.id,
                reply,
            })
            .await?;
        Ok(closed.to_string())
    }

    #[tool(description = "Select a rab-browser tab by numeric ID")]
    async fn select_tab(&self, params: Parameters<TabParams>) -> Result<String, ErrorData> {
        let selected = self
            .request(|reply| McpRequest::SelectTab {
                id: params.0.id,
                reply,
            })
            .await?;
        Ok(selected.to_string())
    }

    #[tool(description = "List rab-browser tabs, including URL, title, and active state")]
    async fn list_tabs(&self) -> Result<String, ErrorData> {
        let tabs = self.request(|reply| McpRequest::ListTabs { reply }).await?;
        serde_json::to_string(&tabs)
            .map_err(|error| ErrorData::internal_error(error.to_string(), None))
    }

    #[tool(description = "Go back in the active rab-browser tab history")]
    async fn go_back(&self) -> Result<String, ErrorData> {
        let moved = self.request(|reply| McpRequest::GoBack { reply }).await?;
        Ok(moved.to_string())
    }

    #[tool(description = "Go forward in the active rab-browser tab history")]
    async fn go_forward(&self) -> Result<String, ErrorData> {
        let moved = self
            .request(|reply| McpRequest::GoForward { reply })
            .await?;
        Ok(moved.to_string())
    }

    #[tool(description = "Reload the active rab-browser tab")]
    async fn reload(&self) -> Result<String, ErrorData> {
        self.request(|reply| McpRequest::Reload { reply })
            .await?
            .map_err(|message| ErrorData::internal_error(message, None))?;
        Ok("ok".to_owned())
    }

    #[tool(description = "Get HTML from the active or specified rab-browser tab")]
    async fn get_dom(&self, params: Parameters<SelectorParams>) -> Result<String, ErrorData> {
        let selector = js_string(params.0.selector.as_deref().unwrap_or("html"))?;
        let script = wrapped_script(&format!(
            "const el=document.querySelector({selector});\
             if(!el) throw new Error('element not found');\
             return el.outerHTML;"
        ));
        self.eval(params.0.target, script).await
    }

    #[tool(description = "Get text from the active or specified rab-browser tab")]
    async fn get_text(&self, params: Parameters<SelectorParams>) -> Result<String, ErrorData> {
        let selector = js_string(params.0.selector.as_deref().unwrap_or("body"))?;
        let script = wrapped_script(&format!(
            "const el=document.querySelector({selector});\
             if(!el) throw new Error('element not found');\
             return el.innerText;"
        ));
        self.eval(params.0.target, script).await
    }

    #[tool(description = "Execute JavaScript in the active or specified rab-browser tab")]
    async fn evaluate(&self, params: Parameters<EvaluateParams>) -> Result<String, ErrorData> {
        let source = js_string(&params.0.script)?;
        let script = wrapped_script(&format!("return await eval({source});"));
        self.eval(params.0.target, script).await
    }

    #[tool(description = "Click an element by CSS selector in a rab-browser tab")]
    async fn click(&self, params: Parameters<ClickParams>) -> Result<String, ErrorData> {
        let selector = js_string(&params.0.selector)?;
        let script = wrapped_script(&format!(
            "const el=document.querySelector({selector});\
             if(!el) throw new Error('element not found');\
             el.click(); return 'ok';"
        ));
        self.eval(params.0.target, script).await
    }

    #[tool(
        name = "type",
        description = "Replace an input value and dispatch input/change events"
    )]
    async fn r#type(&self, params: Parameters<TypeParams>) -> Result<String, ErrorData> {
        let selector = js_string(&params.0.selector)?;
        let text = js_string(&params.0.text)?;
        let script = wrapped_script(&format!(
            "const el=document.querySelector({selector});\
             if(!el) throw new Error('element not found');\
             el.focus(); el.value={text};\
             el.dispatchEvent(new Event('input',{{bubbles:true}}));\
             el.dispatchEvent(new Event('change',{{bubbles:true}}));\
             return 'ok';"
        ));
        self.eval(params.0.target, script).await
    }
}

fn js_string(value: &str) -> Result<String, ErrorData> {
    serde_json::to_string(value).map_err(|error| ErrorData::invalid_params(error.to_string(), None))
}

fn wrapped_script(body: &str) -> String {
    format!("(async()=>{{try{{{body}}}catch(e){{return 'ERR:'+(e?.message??String(e));}}}})()")
}

#[tool_handler]
impl ServerHandler for BrowserMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(
            "Control the visible tabs of the running rab-browser Attached-mode process.",
        )
    }
}

/// Starts the stdio MCP server on a dedicated current-thread Tokio runtime.
pub fn spawn(dispatcher: Arc<dyn RequestDispatcher>) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => {
                eprintln!("failed to start rab-browser MCP runtime: {error}");
                return;
            }
        };
        runtime.block_on(async move {
            let server = BrowserMcpServer::new(dispatcher);
            match server.serve(rmcp::transport::stdio()).await {
                Ok(service) => {
                    if let Err(error) = service.waiting().await {
                        eprintln!("rab-browser MCP server stopped: {error}");
                    }
                }
                Err(error) => eprintln!("failed to serve rab-browser MCP stdio: {error}"),
            }
        });
    })
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    struct MockDispatcher {
        handler: Box<dyn Fn(McpRequest) -> Result<(), DispatchError> + Send + Sync>,
    }

    impl RequestDispatcher for MockDispatcher {
        fn dispatch(&self, request: McpRequest) -> Result<(), DispatchError> {
            (self.handler)(request)
        }
    }

    fn server(
        handler: impl Fn(McpRequest) -> Result<(), DispatchError> + Send + Sync + 'static,
    ) -> BrowserMcpServer {
        BrowserMcpServer::new(Arc::new(MockDispatcher {
            handler: Box::new(handler),
        }))
    }

    #[tokio::test]
    async fn navigate_converts_tool_params_to_request() {
        let seen = Arc::new(Mutex::new(None));
        let captured = Arc::clone(&seen);
        let server = server(move |request| {
            let McpRequest::Navigate { url, reply } = request else {
                panic!("unexpected request");
            };
            *captured.lock().unwrap() = Some(url);
            reply.send(Ok(())).unwrap();
            Ok(())
        });

        assert_eq!(
            server
                .navigate(Parameters(UrlParams {
                    url: "example.com".to_owned(),
                }))
                .await
                .unwrap(),
            "ok"
        );
        assert_eq!(seen.lock().unwrap().as_deref(), Some("example.com"));
    }

    #[test]
    fn registers_the_phase_three_tool_set() {
        let mut names = BrowserMcpServer::tool_router()
            .list_all()
            .into_iter()
            .map(|tool| tool.name.into_owned())
            .collect::<Vec<_>>();
        names.sort();
        assert_eq!(
            names,
            [
                "click",
                "close_tab",
                "evaluate",
                "get_dom",
                "get_text",
                "go_back",
                "go_forward",
                "list_tabs",
                "navigate",
                "new_tab",
                "reload",
                "select_tab",
                "type",
            ]
        );
    }

    #[tokio::test]
    async fn javascript_tools_use_eval_requests() {
        let server = server(|request| {
            let McpRequest::Eval {
                target,
                script,
                reply,
            } = request
            else {
                panic!("unexpected request");
            };
            assert_eq!(target, Some(7));
            assert!(script.contains("querySelector"));
            assert!(script.contains("try{"));
            reply.send(Ok("ok".to_owned())).unwrap();
            Ok(())
        });

        let result = server
            .click(Parameters(ClickParams {
                selector: "#submit".to_owned(),
                target: Some(7),
            }))
            .await
            .unwrap();
        assert_eq!(result, "ok");
    }

    #[tokio::test]
    async fn reports_dispatch_and_response_errors() {
        let dispatch_error = server(|_| Err(DispatchError::LoopClosed))
            .list_tabs()
            .await
            .unwrap_err();
        assert!(dispatch_error.message.contains("event loop is closed"));

        let dropped = server(|request| {
            drop(request);
            Ok(())
        })
        .list_tabs()
        .await
        .unwrap_err();
        assert!(dropped.message.contains("dropped"));
    }

    #[tokio::test]
    async fn times_out_when_browser_does_not_reply() {
        let held = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&held);
        let error = server(move |request| {
            captured.lock().unwrap().push(request);
            Ok(())
        })
        .list_tabs()
        .await
        .unwrap_err();
        assert!(error.message.contains("timed out"));
    }
}
