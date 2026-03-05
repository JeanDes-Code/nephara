# Nephara — Claude Code Project Context

## What Is This?

Nephara is a text-based world simulation where AI agents (embodied by small local LLMs via Ollama) inhabit a shared village, perceive their surroundings, and take actions driven by needs, personality, and capabilities. It features a Kabbalistic-inspired freeform magic system where spoken intent always succeeds but words carry all their semantic meanings.

**Read the full spec at `spec/world-sim-mvp-spec.md` before making any architectural decisions.**

## Tech Stack

- **Language:** Rust (stable toolchain)
- **Async runtime:** Tokio
- **LLM serving:** Ollama via HTTP (localhost:11434)
- **Config:** TOML (`config/world.toml`)
- **OS:** NixOS — all dependencies declared in `flake.nix`
- **GPU:** AMD Vega Frontier Edition (ROCm), but code is GPU-agnostic

## Architecture Principles

1. **LLM backend is behind a trait** (`LlmBackend`). Implementations: `OllamaBackend`, `MockBackend` (random valid actions for testing). The simulation must run fully with MockBackend for testing without an LLM.
2. **All tunable parameters live in `config/world.toml`**, not hardcoded. Decay rates, DCs, restoration amounts, tick counts — everything configurable without recompilation.
3. **The simulation must never crash due to LLM output.** Parse with cascading fallbacks (JSON → code fence extraction → regex → default wander action). Log failures, don't panic.
4. **Soul seed files are canonical.** Agents are initialized from `souls/*.seed.md` files (markdown with YAML frontmatter). These are immutable — never written to by code.
5. **Journals are append-only.** `souls/*.journal.md` files get new entries appended after each run. Never overwrite.

## Key Files

- `spec/world-sim-mvp-spec.md` — the full spec (READ THIS FIRST)
- `config/world.toml` — all tunable world parameters
- `souls/*.seed.md` — entity definitions (parse these at startup)
- `souls/*.journal.md` — living chronicles (append after runs)
- `rituals/summoning.md` — the prompt used to create entities (reference only)

## Source Layout

```
src/
  main.rs    — CLI (clap), initialization, run loop
  world.rs   — World struct, locations, tick cycle, day/night
  agent.rs   — Agent struct, needs, attributes, memory buffer
  action.rs  — Action enum, d20 resolution, outcome tiers
  magic.rs   — Cast Intent flow, Interpreter prompt, response parsing
  llm.rs     — LlmBackend trait, OllamaBackend, MockBackend
  config.rs  — TOML deserialization into typed config struct
  soul.rs    — Parse soul seed markdown (YAML frontmatter + body sections)
  log.rs     — Tick log formatting (stdout + file), journal writing, state dumps
```

## Conventions

- Use `tracing` for all logging, not `println!` (except for the tick log output which goes to both stdout and file)
- Use `Result<T, Box<dyn Error>>` or a custom error enum — never unwrap in non-test code
- Serialize all world state types with serde (needed for JSON state dumps)
- Agent attribute scores must sum to 30 — validate at soul seed parse time
- Needs are clamped to 0.0..=100.0 after every modification
- Action resolution uses d20 rolls for skill-checked actions; magic always succeeds

## CLI Interface

```
nephara [OPTIONS]

Options:
  --ticks <N>         Number of ticks to simulate (default: from config)
  --llm <BACKEND>     LLM backend: ollama, mock (default: ollama)
  --llm-url <URL>     Override Ollama URL
  --model <MODEL>     Override model name
  --config <PATH>     Config file path (default: config/world.toml)
  --souls <DIR>       Soul seeds directory (default: souls/)
  --verbose           Enable debug logging
```

## Things NOT To Do

- Don't add a web UI — this is terminal-based
- Don't use a database — JSON files and markdown are the persistence layer
- Don't add inventory, crafting, relationships, or events — those are post-MVP
- Don't make LLM calls synchronous — use async even though agents act sequentially (keeps the option open for parallel agent calls later)
- Don't modify soul seed files programmatically — they are immutable artifacts
- Don't hardcode world parameters — if it's a number that might need tuning, put it in world.toml
