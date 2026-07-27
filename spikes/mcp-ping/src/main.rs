use rmcp::{
    ServerHandler, ServiceExt,
    handler::server::router::tool::ToolRouter,
    model::{ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
};

#[allow(dead_code)]
#[derive(Clone)]
struct PingServer {
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl PingServer {
    fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    #[tool(description = "Return pong to verify that the rab-browser MCP server is alive")]
    fn ping(&self) -> String {
        "pong".to_owned()
    }
}

#[tool_handler]
impl ServerHandler for PingServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("Phase 0 rmcp stdio smoke-test server")
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let service = PingServer::new().serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;
    Ok(())
}
