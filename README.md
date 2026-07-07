# dogma-gateway

> The network harness of the [Dogma](https://github.com/dogmalab/.github) platform.
> The only network listener. Bridges HTTP clients to the air-gapped core via
> IPC pipes and mmap. Stateless. Tiny binary.

**Read first:** the [Dogma Manifesto](https://github.com/dogmalab/.github/blob/main/MANIFESTO.md)
explains why this exists and what it is for. This document is the
network-harness-specific README.

---

## What dogma-gateway is

`dogma-gateway` is the **network harness** of the Dogma platform. It
is a standalone binary crate that runs as an HTTP server with three
responsibilities:

1. **REST ingress** with strict JSON validation
   (`#[serde(deny_unknown_fields)]`).
2. **SSE streaming proxy** that bridges HTTP clients to
   `dogma-agent` via anonymous OS pipes (stdin/stdout).
3. **RAG orchestrator** that combines a local vector search against
   `dogma-vdb` (mmap) with an LLM inference call to the agent.

The gateway has **no access to system shells**, **no local session
storage**, and **no reasoning loop**. It is a pure routing and
validation layer — the network boundary of the air-gapped core.

---

## Component Definition

`dogma-gateway` is the network perimeter component of the **Dogma** ecosystem.
It is a standalone **binary crate** that runs as an HTTP server with exactly
three responsibilities:

1. **REST ingress** — Validates incoming JSON payloads at the boundary with
   zero tolerance for malformed or unexpected fields
   (`#[serde(deny_unknown_fields)]`).
2. **SSE streaming proxy** — Bridges Web/HTTP clients to `dogma-agent` (v2)
   via anonymous OS pipes (stdin/stdout).  The agent itself never binds a
   socket.
3. **RAG orchestrator** — Combines a local vector search against
   `dogma-vdb` (v1) with an LLM-inference call to the agent in a single
   pass, returning retrieved contexts and the synthesised answer.

The gateway has **no access to system shells**, **no local session storage**,
and **no reasoning loop** — it is a pure routing and validation layer.

---

## Topology of Isolation

```
                        ╔═══════════════════════════════════╗
                        ║         EXTERNAL NETWORK          ║
                        ║       (Internet / LAN / K8s)      ║
                        ╚═══════════════╦═══════════════════╝
                                        │
                          HTTP REST / SSE
                          POST /v1/vector/search
                          POST /v1/agent/stream
                          POST /v1/rag
                                        │
                        ┌───────────────┴───────────────┐
                        │      dogma-gateway (v1)        │
                        │                                │
                        │  • JSON validation on ingress  │
                        │  • SSE termination             │
                        │  • Rate limiting (future)      │
                        │  • 0 system calls (no sh)      │
                        └───────────────┬───────────────┘
                                        │
                    ┌───────────────────┼───────────────────┐
                    │                   │                   │
               IPC Pipes           mmap I/O            IPC Pipes
               (stdin/out)         (shared mem)        (stdin/out)
                    │                   │                   │
                    ▼                   ▼                   ▼
            ┌──────────────┐  ┌──────────────┐  ┌──────────────────┐
            │ dogma-agent  │  │  dogma-vdb   │  │  dogma-agent     │
            │ (v2)         │  │  (v1)        │  │  (v2)            │
            │ [isolated]   │  │  [local]     │  │  [isolated]      │
            │ 0 network    │  │  0 network   │  │  0 network       │
            └──────────────┘  └──────────────┘  └──────────────────┘
```

**Key architectural rule:** `dogma-agent` (v2) is completely network-isolated.
It has zero inbound connections. All interaction happens through anonymous
pipes managed by `dogma-gateway`.

---

## Endpoint Contract

| Method | Path                | Content-Type (req → res) | Description                                               |
|--------|---------------------|--------------------------|-----------------------------------------------------------|
| POST   | `/v1/vector/search` | `application/json` → `application/json` | Vector similarity search against `dogma-vdb` (v1).        |
| POST   | `/v1/agent/stream`  | `application/json` → `text/event-stream` | IPC proxy to `dogma-agent` (v2). Returns NDJSON over SSE. |
| POST   | `/v1/rag`           | `application/json` → `application/json` | Unified RAG: VDB search + LLM synthesis in one call.     |

### Strict JSON rules

- All request bodies use `#[serde(deny_unknown_fields)]` — extra fields are
  **rejected** with `400 BAD_REQUEST` and error code `"PARSE_ERROR"`.
- Empty query vectors and blank questions are rejected at the handler level
  before any downstream call.
- All errors are returned as `{"error": "<CODE>", "message": "..."}`
  with the corresponding HTTP status code.

---

## Build & Run

```sh
# Development (debug)
cargo build

# Production (optimised for size)
cargo build --release

# Run
RUST_LOG=dogma_gateway=info cargo run --release
```

### Release profile

This crate's `Cargo.toml` enforces:

| Setting      | Value   |
|--------------|---------|
| `opt-level`  | `"z"`   |
| `strip`      | `true`  |
| `lto`        | `true`  |

This keeps the final binary well under 20 MB on Alpine Linux.

---

## Crate Dependencies

| Dependency              | Purpose                              |
|-------------------------|--------------------------------------|
| `axum`                  | HTTP framework (routing, extractors) |
| `tokio`                 | Async runtime (full features)        |
| `serde` / `serde_json`  | Strict JSON serialisation            |
| `tracing` / `tracing-subscriber` | Structured logging to stderr  |
| `thiserror`             | Typed error derive macros            |
| `futures`               | `Stream` trait and combinators (SSE) |
| `dogma-v2-common`       | Shared error codes and types         |

---

## Status

**v0.1.0** — In development (v2 ecosystem). All endpoints respond with stub/simulated
data. Production integration with `dogma-vdb` (mmap) and `dogma-agent` (IPC
pipes) is tracked in the project backlog.

This crate is part of the **Dogma** ecosystem.
See [`dogmalab/dogma-agent`](https://github.com/dogmalab/dogma-agent) and
[`dogmalab/dogma-vdb`](https://github.com/dogmalab/dogma-vdb) for sibling components.
