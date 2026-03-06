# Testing and Evaluating Local Models in Nephara

A practical guide for benchmarking and qualitatively evaluating local Ollama models against the Nephara simulation.

## 1. Pull the Model

```sh
ollama pull qwen3.5:9b
```

Confirm it appears in `ollama list` before proceeding.

## 2. Quick Smoke Test (10 ticks)

```sh
nix develop --command cargo run -- --model qwen3.5:9b --ticks 10 --no-tui
```

Look for:
- No panics or crashes
- All agents acting (not defaulting to Wander every tick)
- Reasonable action variety (not all "eat" or "rest")

## 3. Quantitative Benchmark

```sh
nix develop --command cargo run -- bench \
  --models "gemma3:4b,qwen3.5:9b" \
  --samples 10 \
  --output bench-results.json
```

Four metrics per prompt type (action / narrative / interpreter / planning):

| Metric | What it measures |
|---|---|
| `parse_rate` | Fraction of responses that parsed successfully — most important |
| `avg_latency_ms` | Speed matters for multi-agent ticks |
| `avg_chars` | Flags runaway verbosity or truncated output |

**Good bar:** action parse >= 90%, all latencies < 3000ms on local hardware.

If `parse_rate` for action prompts falls below 80%, the model is unreliable for real runs — agents will default to Wander too often.

## 4. Qualitative Inspection with --debug-llm

```sh
nix develop --command cargo run -- --model qwen3.5:9b --ticks 20 --no-tui --debug-llm
# Output written to: runs/{ID}/llm_debug.md
```

Read the debug file and check each prompt type:

**Action prompts**
- Does the model stay in JSON? Does it use valid action names?
- Watch for extra prose before/after the JSON object.

**Thinking blocks (Qwen3-specific)**
- Qwen3 emits `<think>...</think>` before its answer. These are stripped automatically by `strip_thinking_tags()` in `src/action.rs`.
- Verify the content after stripping is still valid JSON.

**Interpreter prompts**
- Is the JSON well-formed?
- Are `need_changes` values plausible floats (e.g., -5.0 to +10.0)?

**Narrative prompts**
- Are the 2-3 sentences evocative and relevant to the action taken?

**Planning prompts**
- Are the 1-2 sentences personal and in-character?

## 5. Qwen3-Specific Notes

- Qwen3 models output chain-of-thought in `<think>` tags before the answer. Nephara strips these automatically (`strip_thinking_tags()` in `src/action.rs`). The fallback parse chain (JSON -> code fence extraction -> regex -> default wander) handles any residual issues.
- Qwen3 has strong instruction-following, so JSON parse rates should be high.
- It may be slower than gemma3 on CPU. Check latency before using it for all ticks.
- Consider using it only as the `smart_model` (planning/reflection) while keeping a faster model for action selection.

## 6. Dual-Model Configuration

Edit `config/world.toml`:

```toml
[llm]
model         = "gemma3:4b"      # fast model for action selection every tick
smart_model   = "qwen3.5:9b"    # deeper model for planning/reflection/desires
# smart_ollama_url = "http://localhost:11435"  # only if running a separate Ollama instance
```

**Smart model is used for:** morning intentions, end-of-day reflection, daily desires, end-of-run desires.

**Main model is used for:** action selection every tick, narrator descriptions.

This split lets you get fast tick cycles while still benefiting from a more capable model on the prompts where quality matters most.

## 7. Comparing Models Side-by-Side

```sh
nix develop --command cargo run -- bench \
  --models "gemma3:4b,qwen3.5:9b" \
  --samples 20 \
  --output comparison.json

cat comparison.json | python3 -m json.tool
```

Focus on `parse_rate` differences in the `action` prompt type first — that has the most impact on simulation quality. Then compare `avg_latency_ms` to understand the speed tradeoff.
