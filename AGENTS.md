# AGENTS.md — dogma-gateway Development Rules

> **Immutable expansion boundaries for AI agents working on this crate.**
> Every change must pass the checklist below before commit.
>
> **Standalone repository** at `https://github.com/dogmalab/dogma-gateway`.
> Part of the Dogma ecosystem — works alongside `dogma-agent` and `dogma-vdb`.

---

## 1. Expansion Limits

### What the gateway DOES

| Concern | Description |
|---------|-------------|
| **JSON validation at the edge** | Every incoming payload is parsed with `#[serde(deny_unknown_fields)]`. Reject malformed data with `400 BAD_REQUEST` before any downstream call. |
| **SSE protocol termination** | Stream events from `dogma-agent` (v2) to HTTP clients using Server-Sent Events. Manage keep-alive, reconnection hints, and graceful shutdown. |
| **Low-latency routing** | Forward requests to the correct backend (`dogma-vdb` mmap vs `dogma-agent` IPC) with minimal overhead. Head-of-line blocking must be avoided. |
| **Typed error mapping** | Every network error, I/O error, and parse failure is converted to `GatewayError` and returned as a structured JSON error response. No `unwrap()` in handlers. |

### What the gateway MUST NOT do

| Prohibition | Rationale |
|-------------|-----------|
| **Execute system scripts** | No `std::process::Command` to Bash, Python, or any shell. The gateway is a pure Rust routing layer. Script execution belongs in the agent. |
| **Manage RSI / reasoning loop** | The gateway does not orchestrate chain-of-thought, tool-call loops, or agent state machines. It is a stateless proxy. |
| **Store local session files** | All persistent state belongs in `dogma-vdb` (v1). The gateway is ephemeral — no filesystem writes beyond the binary itself. |
| **Dial outbound network connections** | The gateway only listens; it never initiates HTTP/TCP connections to external services. Backend communication is via mmap or anonymous pipes. |
| **Expose internal crate types** | Public types are defined in this crate's handler modules. `dogma-v2-common` error codes are used internally but never leaked into the API contract. |

---

## 2. Edge Quality Checklist

Before committing any change to `dogma-gateway`, verify each item:

- [ ] **0 unnecessary dynamic allocations** — Prefer stack-local types,
  `Cow`, and small-vector optimisation. Every `Vec` / `String` allocation
  must be justified by the data size.

- [ ] **Overflow mitigation via native types** — All numeric bounds are
  enforced by Rust's type system (no `usize` ↔ `u64` casts without
  `TryFrom`). Vector dimension checks happen at deserialisation time.

- [ ] **Handler error path uses `?` exclusively** — No `unwrap()`,
  `expect()`, or `panic!()` in handler logic. Use `GatewayError` and the
  `?` operator. Panics are acceptable only in `main()` initialisation.

- [ ] **Stream back-pressure handled** — SSE streams must not buffer
  indefinitely. Use bounded channels (`broadcast`, `mpsc`) with sensible
  capacity limits. Client disconnection is detected and cleans up resources.

- [ ] **Zero unsafe code** — The `#![deny(unsafe_code)]` lint is set at the
  crate root. Any `unsafe` block is a compile error. If truly required (e.g.
  mmap), it must be isolated in a `#[allow(unsafe_code)]` module with a
  safety audit comment.

---

## 3. Integration Testing Guidelines

- All three endpoints (`/v1/vector/search`, `/v1/agent/stream`,
  `/v1/rag`) must have at least one **happy-path** and **sad-path** test.
- SSE stream tests should verify:
  - Correct `Content-Type: text/event-stream` header.
  - At least one data frame is received before the stream closes.
  - Client disconnect triggers clean task cancellation.
- Use `axum::body::Body` and `axum::http::Request` builders for
  in-process integration tests (no external server required).

---

## 4. Cargo Convention

- Dependencies are specified with minimum SemVer constraints only
  (e.g. `"0.7"`, not `"=0.7.5"`) — let Cargo resolve patches.
- Crate-level metadata (`version`, `edition`, `license`) is defined
  in this crate's `Cargo.toml`.
- The `[profile.release]` block is defined at the workspace level
  in `dogma-agent`; this standalone crate defines its own explicit
  release profile when size optimisation is required.

---

## 5. Future-Proofing Notes

- The `dogma-vdb` integration will use `memmap2` for zero-copy reads.
  Do **not** add `memmap2` to this crate's dependencies yet — it will
  arrive in a separate crate that this one depends on.
- IPC with `dogma-agent` (v2) will use `tokio::process::{Command, Child}`
  with `Stdio::piped()`. The skeleton in `agent_stream` is ready for this.
- Rate limiting and auth (API keys, JWT) will be added as middleware —
  the `Router` is already set up for `.layer()` additions.
