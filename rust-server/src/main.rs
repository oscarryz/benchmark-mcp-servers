use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters}, model::*,
    schemars,
    tool,
    tool_handler, tool_router, transport::streamable_http_server::{
        session::local::LocalSessionManager, StreamableHttpService,
    }, ErrorData as McpError,
    ServerHandler,
};
use serde::{Deserialize, Serialize};
use socket2::{Domain, Protocol, Socket, Type};
use std::sync::Arc;
use tower_http::cors::CorsLayer;

// ── Input structs ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct FibonacciInput {
    pub n: u8,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct HttpGetInput {
    pub endpoint: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ProcessJsonInput {
    pub data: serde_json::Value,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DatabaseQueryInput {
    pub query: String,
    pub delay_ms: u64,
}

// ── Output structs ────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct FibonacciOutput {
    input: u8,
    result: i64,
    server_type: String,
}

#[derive(Debug, Serialize)]
pub struct FetchDataOutput {
    url: String,
    status_code: u16,
    response_time_ms: u128,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    server_type: String,
}

#[derive(Debug, Serialize)]
pub struct ProcessJsonOutput {
    original_keys: Vec<String>,
    transformed_data: serde_json::Value,
    server_type: String,
}

#[derive(Debug, Serialize)]
pub struct DatabaseOutput {
    query: String,
    delay_ms: u64,
    timestamp: String,
    server_type: String,
}

// ── Server struct ─────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct RustMCPServer {
    tool_router: ToolRouter<Self>,
    http_client: reqwest::Client,
}

// ── Tool implementations ──────────────────────────────────────────────────────

#[tool_router]
impl RustMCPServer {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
            http_client: reqwest::Client::new(),
        }
    }

    #[tool(description = "Calcula o N-ésimo número de Fibonacci de forma recursiva")]
    async fn calculate_fibonacci(
        &self,
        Parameters(FibonacciInput { n }): Parameters<FibonacciInput>,
    ) -> Result<CallToolResult, McpError> {
        if n > 40 {
            return Err(McpError::invalid_params("n must be between 0 and 40", None));
        }

        let output = FibonacciOutput {
            input: n,
            result: compute_fibonacci(n as i64),
            server_type: "rust".to_string(),
        };

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string(&output)
                .map_err(|e| McpError::internal_error(e.to_string(), None))?,
        )]))
    }

    #[tool(description = "Faz uma requisição HTTP GET para uma API externa")]
    async fn fetch_external_data(
        &self,
        Parameters(HttpGetInput { endpoint }): Parameters<HttpGetInput>,
    ) -> Result<CallToolResult, McpError> {
        let start = std::time::Instant::now();
        let result = self.http_client.get(&endpoint).send().await;
        let response_time_ms = start.elapsed().as_millis();

        let output = match result {
            Ok(response) => FetchDataOutput {
                url: endpoint,
                status_code: response.status().as_u16(),
                response_time_ms,
                error: None,
                server_type: "rust".to_string(),
            },
            Err(e) => FetchDataOutput {
                url: endpoint,
                status_code: 0,
                response_time_ms,
                error: Some(e.to_string()),
                server_type: "rust".to_string(),
            },
        };

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string(&output)
                .map_err(|e| McpError::internal_error(e.to_string(), None))?,
        )]))
    }

    #[tool(description = "Recebe um JSON, valida e transforma (uppercase em campos string)")]
    async fn process_json_data(
        &self,
        Parameters(ProcessJsonInput { data }): Parameters<ProcessJsonInput>,
    ) -> Result<CallToolResult, McpError> {
        let original_keys: Vec<String> = data
            .as_object()
            .ok_or_else(|| McpError::invalid_params("Input must be a JSON object", None))?
            .keys()
            .cloned()
            .collect();

        let transformed = transform_strings(&data);

        let output = ProcessJsonOutput {
            original_keys,
            transformed_data: transformed,
            server_type: "rust".to_string(),
        };

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string(&output)
                .map_err(|e| McpError::internal_error(e.to_string(), None))?,
        )]))
    }

    #[tool(description = "Simula uma query de banco de dados com delay configurável")]
    async fn simulate_database_query(
        &self,
        Parameters(DatabaseQueryInput { query, delay_ms }): Parameters<DatabaseQueryInput>,
    ) -> Result<CallToolResult, McpError> {
        if delay_ms > 5000 {
            return Err(McpError::invalid_params(
                "delay_ms must be between 0 and 5000",
                None,
            ));
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;

        let output = DatabaseOutput {
            query,
            delay_ms,
            timestamp: chrono::Utc::now().to_rfc3339(),
            server_type: "rust".to_string(),
        };

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string(&output)
                .map_err(|e| McpError::internal_error(e.to_string(), None))?,
        )]))
    }
}

// ── ServerHandler ─────────────────────────────────────────────────────────────

#[tool_handler]
impl ServerHandler for RustMCPServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2025_03_26,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "rust-mcp-server".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                title: None,
                website_url: None,
                icons: None,
                description: None,
            },
            instructions: Some("Rust MCP benchmark server with 4 tools.".to_string()),
        }
    }
}

// ── Pure logic ────────────────────────────────────────────────────────────────

fn compute_fibonacci(n: i64) -> i64 {
    if n <= 1 {
        return n;
    }
    let (mut a, mut b) = (0i64, 1i64);
    for _ in 2..=n {
        (a, b) = (b, a + b);
    }
    b
}

fn transform_strings(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::String(s) => serde_json::Value::String(s.to_uppercase()),
        serde_json::Value::Object(map) => {
            let transformed = map
                .iter()
                .map(|(k, v)| (k.clone(), transform_strings(v)))
                .collect();
            serde_json::Value::Object(transformed)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(transform_strings).collect())
        }
        other => other.clone(),
    }
}

// ── Health endpoint ───────────────────────────────────────────────────────────

async fn health() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({
        "status": "ok",
        "server_type": "rust"
    }))
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let server = Arc::new(RustMCPServer::new());

    let service = StreamableHttpService::new(
        move || Ok(Arc::clone(&server)),
        LocalSessionManager::default().into(),
        Default::default(),
    );

    let app = axum::Router::new()
        .nest_service("/mcp", service)
        .route("/health", axum::routing::get(health))
        .route("/", axum::routing::get(|| async { "Rust MCP running" }))
        .layer(CorsLayer::permissive());

    let socket = Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP))?;
    socket.set_reuse_address(true)?;
    socket.set_reuse_port(true)?;
    socket.set_tcp_nodelay(true)?;
    socket.bind(&"0.0.0.0:8084".parse::<std::net::SocketAddr>()?.into())?;
    socket.listen(1024)?;
    socket.set_nonblocking(true)?;
    let listener = tokio::net::TcpListener::from_std(socket.into())?;

    tracing::info!("🚀  MCP server listening on http://0.0.0.0:8084/mcp");
    axum::serve(listener, app).await?;
    Ok(())
}
