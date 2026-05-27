//! dogma-gateway — Edge proxy / reverse proxy for the Dogma ecosystem.
//!
//! Acts as a low-latency network gateway protecting `dogma-agent` (v2) from
//! direct network exposure, while forwarding structured requests to
//! `dogma-vdb` (v1) via memory-mapped I/O.
//!
//! # Safety
//! 0 `unsafe` in this crate. The `#![deny(unsafe_code)]` attribute ensures
//! no accidental escape hatches.

#![deny(unsafe_code)]

mod error;

use axum::{
    Router,
    extract::State,
    response::{
        Json,
        sse::{Event, KeepAlive, Sse},
    },
    routing::post,
};
use error::GatewayError;
use futures::stream::{self, Stream};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tracing_subscriber::EnvFilter;

// ---------------------------------------------------------------------------
// Shared application state
// ---------------------------------------------------------------------------

/// State injected into every handler via Axum's [`State`] extractor.
#[derive(Clone)]
struct AppState {
    /// Broadcast channel for simulating IPC events from `dogma-agent`.
    agent_tx: broadcast::Sender<AgentEvent>,
}

// ---------------------------------------------------------------------------
// Request / response types  (all use `#[serde(deny_unknown_fields)]`)
// ---------------------------------------------------------------------------

/// Request body for `POST /v1/vector/search`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct VectorSearchRequest {
    /// Dense embedding vector to search with.
    query: Vec<f32>,
    /// Number of nearest neighbours to retrieve (default: 10).
    top_k: Option<usize>,
    /// Optional namespace filter.
    namespace: Option<String>,
}

/// Single search result returned by `POST /v1/vector/search`.
#[derive(Serialize)]
struct SearchResult {
    /// Document identifier.
    id: String,
    /// Similarity score (1.0 = exact match).
    score: f32,
    /// Optional metadata payload.
    metadata: Option<serde_json::Value>,
}

/// Response body for `POST /v1/vector/search`.
#[derive(Serialize)]
struct VectorSearchResponse {
    /// Ordered list of results (highest score first).
    results: Vec<SearchResult>,
    /// Wall-clock time of the search in milliseconds.
    took_ms: u64,
}

/// Request body for `POST /v1/agent/stream`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct AgentStreamRequest {
    /// Unique session identifier to associate with the agent.
    session_id: String,
    /// Incoming message or tool-call payload.
    message: serde_json::Value,
}

/// Event emitted by the agent on the SSE stream.
#[derive(Serialize, Clone)]
struct AgentEvent {
    /// Event type (e.g. "token", "tool_call", "error", "done").
    event: String,
    /// JSON-encoded payload for the event.
    data: String,
}

/// Request body for `POST /v1/rag`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct RagRequest {
    /// Natural-language question to answer.
    question: String,
    /// Number of context documents to retrieve from VDB.
    top_k: Option<usize>,
    /// Optional namespace to scope the VDB search.
    namespace: Option<String>,
}

/// Response body for `POST /v1/rag`.
#[derive(Serialize)]
struct RagResponse {
    /// Retrieved context passages from `dogma-vdb` (v1).
    contexts: Vec<String>,
    /// Final answer synthesised by the agent LLM.
    answer: String,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `POST /v1/vector/search`
///
/// Simulates a direct vector-similarity query against `dogma-vdb` (v1).
/// In production this will use memory-mapped I/O to read the HNSW / IVF-PQ
/// index directly from the shared-memory region.
async fn vector_search(
    State(_state): State<AppState>,
    Json(payload): Json<VectorSearchRequest>,
) -> Result<Json<VectorSearchResponse>, GatewayError> {
    // Guard: vector must not be empty.
    if payload.query.is_empty() {
        return Err(GatewayError::BadRequest(
            "query vector must not be empty".into(),
        ));
    }

    // TODO(perf): replace with real dogma-vdb mmap query (HNSW / IVF-PQ).
    let results = vec![
        SearchResult {
            id: "doc-sample-1".into(),
            score: 0.95,
            metadata: Some(serde_json::json!({"source": "alice-in-wonderland"})),
        },
        SearchResult {
            id: "doc-sample-2".into(),
            score: 0.87,
            metadata: None,
        },
    ];

    Ok(Json(VectorSearchResponse {
        results,
        took_ms: 3,
    }))
}

/// `POST /v1/agent/stream`
///
/// SSE proxy for IPC communication with `dogma-agent` (v2).
///
/// The handler accepts a JSON request body containing the session identifier
/// and message payload.  It subscribes to the internal broadcast channel
/// and streams [`AgentEvent`] items as Server-Sent Events.
///
/// # Production path
/// - Spawn a dedicated `tokio::process::Command` with anonymous pipes.
/// - Write the serialised request to the agent's `stdin`.
/// - Read NDJSON lines from the agent's `stdout` and emit each as an SSE event.
async fn agent_stream(
    State(state): State<AppState>,
    Json(_payload): Json<AgentStreamRequest>,
) -> Result<Sse<impl Stream<Item = Result<Event, GatewayError>>>, GatewayError> {
    let rx = state.agent_tx.subscribe();

    let stream = stream::unfold(rx, |mut rx| async move {
        match rx.recv().await {
            Ok(event) => {
                let sse = Event::default().event(event.event).data(event.data);
                Some((Ok(sse), rx))
            }
            Err(broadcast::error::RecvError::Closed) => None,
            Err(broadcast::error::RecvError::Lagged(_)) => {
                // Client dropped too many events; recover gracefully.
                Some((Ok(Event::default().event("error").data("{}")), rx))
            }
        }
    });

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

/// `POST /v1/rag`
///
/// Unified RAG orchestrator: single-pass vector search + LLM synthesis.
///
/// 1. Executes a vector-similarity query against `dogma-vdb` (v1).
/// 2. Injects the retrieved contexts into an LLM-inference request
///    forwarded to `dogma-agent` (v2) via IPC.
/// 3. Returns both the raw context chunks and the synthesised answer.
async fn rag(
    State(_state): State<AppState>,
    Json(payload): Json<RagRequest>,
) -> Result<Json<RagResponse>, GatewayError> {
    // Guard: question must not be blank.
    if payload.question.trim().is_empty() {
        return Err(GatewayError::BadRequest(
            "question must not be empty".into(),
        ));
    }

    // Step 1 — vector search (simulated)
    // TODO(perf): replace with real dogma-vdb mmap query.
    let contexts = vec![
        "Alice was beginning to get very tired of sitting by her sister on the bank...".into(),
        "...and of having nothing to do: once or twice she had peeped into the book her sister was reading...".into(),
    ];

    // Step 2 — LLM synthesis (simulated IPC call)
    // TODO: forward context + question via anonymous pipe to dogma-agent.

    Ok(Json(RagResponse {
        answer: format!(
            "Based on the provided context, here is the answer to: '{}'",
            payload.question
        ),
        contexts,
    }))
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Application entry point.
///
/// Initialises tracing (to stderr), builds shared state, registers all
/// routes, and starts the Axum HTTP server on port 8080.
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialise structured logging (stderr — never pollute stdout).
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    tracing::info!("starting dogma-gateway v0.1.0");

    // Shared broadcast channel for agent events.
    let (agent_tx, _) = broadcast::channel::<AgentEvent>(256);
    let state = AppState { agent_tx };

    // Build router with all v1 endpoints.
    let app = Router::new()
        .route("/v1/vector/search", post(vector_search))
        .route("/v1/agent/stream", post(agent_stream))
        .route("/v1/rag", post(rag))
        .with_state(state);

    // Bind to all interfaces.
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    tracing::info!("listening on 0.0.0.0:8080");

    axum::serve(listener, app).await?;

    Ok(())
}
