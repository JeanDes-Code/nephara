# Nephara — World Simulation MVP Spec

> **Status:** Draft v0.3 — Ready for Implementation
> **Author:** Archwizard + Nabu
> **Date:** 2026-03-05
> **Stack:** Rust, NixOS (flake.nix), Ollama (ROCm/CPU), small local LLMs
> **Codename:** Nephara (from *Nephesh* + *Beriah* — "soul-creation")

---

## 1. Vision

A text-based world simulation where multiple AI agents (embodied by small local LLMs) inhabit a shared space, perceive their surroundings, and take actions driven by their needs, personality, and capabilities. The focus is on **emergent behavior** and **interesting outcomes** rather than realism. D&D ability checks meet Animal Crossing vibes meet The Sims' needs system, with a Kabbalistic freeform magic system where words carry all their meanings.

### MVP Goal

3 agents (the Founding Three, summoned via ritual prompt against Opus 4.6) living in a small village, taking turns acting in a tick-based loop. Each agent perceives its local environment, decides on an action, and the world resolves that action with randomness and modifiers.

### Success Criteria

Run a 3-day simulation (144 ticks) where all three agents survive, take diverse actions, interact with each other at least once, and cast at least one intent each. The tick log tells a story that makes you smile.

---

## 2. Project Structure

```
nephara/
├── CLAUDE.md                     # Claude Code project context
├── flake.nix                     # NixOS dev environment
├── flake.lock
├── Cargo.toml
├── Cargo.lock
├── config/
│   └── world.toml                # Tunable world parameters
├── rituals/
│   └── summoning.md              # The ritual prompt for creating entities
├── souls/
│   ├── [name1].seed.md           # Founding entity 1 — immutable
│   ├── [name1].journal.md        # Living chronicle — appendable
│   ├── [name2].seed.md
│   ├── [name2].journal.md
│   ├── [name3].seed.md
│   └── [name3].journal.md
├── src/
│   ├── main.rs                   # Entry point, CLI, run loop
│   ├── world.rs                  # World state, locations, tick logic
│   ├── agent.rs                  # Agent struct, needs, attributes, memory
│   ├── action.rs                 # Action enum, resolution, d20 rolls
│   ├── magic.rs                  # Intent casting, interpretation
│   ├── llm.rs                    # LLM backend trait + Ollama implementation
│   ├── config.rs                 # TOML config loading
│   ├── soul.rs                   # Soul seed parsing from markdown
│   └── log.rs                    # Tick log formatting, journal writing
├── runs/
│   └── [timestamp]/              # Per-run output
│       ├── tick_log.txt          # Full tick-by-tick narrative
│       └── state_dump.json       # Periodic world state snapshots
└── spec/
    └── world-sim-mvp-spec.md     # This file
```

---

## 3. Environment & Tooling

### 3.1 System

- **OS:** NixOS
- **GPU:** AMD Vega Frontier Edition (16GB VRAM, ROCm)
- **Build/Dev:** `flake.nix` for all dependency declarations
- **LLM serving:** Ollama (ROCm preferred, CPU fallback)
- **Target models:** Gemma 3 4B (primary), Gemma 3 12B Q4 or Qwen 2.5 7B (stretch)

### 3.2 Flake.nix

The flake should provide a dev shell with:

- Rust toolchain (stable, with `cargo`, `clippy`, `rustfmt`)
- Ollama
- System dependencies (`pkg-config`, `openssl-dev` for reqwest TLS)

ROCm + Ollama on NixOS is a known pain point. Strategy: try ROCm first via `nixpkgs-unstable` or overlay. If it doesn't cooperate, fall back to CPU mode (`ollama` without ROCm). The Rust code doesn't care — it just hits an HTTP endpoint. Document whatever works in the README.

### 3.3 Rust Crates

```toml
[dependencies]
tokio = { version = "1", features = ["full"] }
reqwest = { version = "0.12", features = ["json"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
rand = "0.8"
tracing = "0.1"
tracing-subscriber = "0.3"
clap = { version = "4", features = ["derive"] }
async-trait = "0.1"
```

### 3.4 LLM Backend Abstraction

The LLM integration must be behind a trait so we can swap implementations:

```rust
#[async_trait]
trait LlmBackend: Send + Sync {
    async fn generate(&self, prompt: &str, max_tokens: u32) -> Result<String>;
}
```

Implementations:
- `OllamaBackend` — hits `http://localhost:11434/api/generate` (primary)
- `MockBackend` — returns random valid actions (for M1 testing without LLM)
- Future: `OpenAICompatibleBackend` — any endpoint that speaks the OpenAI chat API

Selected via CLI flag: `--llm ollama` (default), `--llm mock`, or `--llm endpoint --llm-url http://...`

Ollama handles GPU/CPU selection internally. If ROCm works, it uses GPU. If not, CPU (slower but functional). The Rust code is agnostic to this.

---

## 4. Configuration (`config/world.toml`)

All tunable world parameters live in a TOML file. No recompilation needed to tweak the simulation.

```toml
[time]
ticks_per_day = 48           # Each tick ~ 30 minutes
day_start_tick = 0           # Tick 0 = dawn
night_start_tick = 32        # Tick 32 = nightfall (~4 PM)

[needs.decay_per_tick]
hunger = 1.5
energy = 1.2
fun = 0.8
social = 0.6
hygiene = 0.3

[needs.initial]
hunger = 80.0
energy = 80.0
fun = 80.0
social = 80.0
hygiene = 80.0

[needs.thresholds]
penalty_mild = 20.0          # Below this: -2 to relevant checks
penalty_severe = 10.0        # Below this: -4 to relevant checks
forced_action = 5.0          # Below this: forced (e.g., forced sleep)

[actions.eat]
hunger_restore = 30.0
dc = 0                       # Auto-success

[actions.cook]
hunger_restore = 45.0
fun_restore = 10.0
dc = 12
attribute = "wit"

[actions.sleep]
duration_ticks = 16
energy_restore_per_tick = 6.25  # 16 * 6.25 = 100 (full refill)
dc = 0

[actions.rest]
energy_restore = 15.0
dc = 0

[actions.forage]
hunger_restore = 20.0
dc = 10
attribute = "grace"

[actions.fish]
hunger_restore = 35.0
fun_restore = 10.0
dc = 12
attribute = "grace"

[actions.exercise]
fun_restore = 15.0
energy_drain = 10.0
dc = 10
attribute = "vigor"

[actions.chat]
social_restore = 20.0
fun_restore = 8.0
dc = 8
attribute = "heart"

[actions.bathe]
hygiene_restore = 50.0
dc = 0

[actions.explore]
fun_restore = 20.0
dc = 12
attribute = "vigor"

[actions.play]
fun_restore = 15.0
dc = 0

[actions.cast_intent]
min_duration_ticks = 1
max_duration_ticks = 4
energy_drain = 8.0
dc = 0                       # Spells always succeed
attribute = "numen"           # Governs clarity, not success

[resolution]
crit_fail = 1
crit_success = 20
dc_easy = 8
dc_medium = 12
dc_hard = 16
night_dc_bonus = 4           # Added to Forage/Explore DCs at night

[memory]
buffer_size = 20

[simulation]
default_run_ticks = 144      # 3 days
state_dump_interval = 48     # Dump every in-game day

[llm]
model = "gemma3:4b"
temperature = 0.7
max_tokens = 150
ollama_url = "http://localhost:11434"
interpreter_max_tokens = 300  # Interpreter needs more room
```

---

## 5. World Model

### 5.1 Space: The Village

A graph of named locations with adjacency. No coordinate geometry.

```
┌─────────┐     ┌──────────┐     ┌───────────┐
│  Forest │─────│  Village │─────│   River   │
│         │     │  Square  │     │           │
└─────────┘     └────┬─────┘     └───────────┘
                     │
              ┌──────┴──────┐
              │             │
        ┌─────┴───┐   ┌────┴────┐
        │  Tavern  │   │  Homes  │
        │          │   │  (x3)   │
        └──────────┘   └─────────┘
```

| Location       | Adjacent To                    | Affordances                              |
|----------------|--------------------------------|------------------------------------------|
| Village Square | Forest, River, Tavern, Homes   | Chat, Exercise, Play, Cast Intent        |
| Tavern         | Village Square                 | Eat, Cook, Chat, Play, Cast Intent       |
| Forest         | Village Square                 | Forage, Explore, Exercise, Cast Intent   |
| River          | Village Square                 | Fish, Bathe, Rest, Cast Intent           |
| Home (x3)      | Village Square                 | Eat, Cook, Sleep, Rest, Cast Intent      |

Each home is assigned to one agent. All locations connect through the Village Square.

### 5.2 Time

**Tick-based.** Each tick ~ 30 minutes. Full day = 48 ticks.

**Action duration:** Most = 1 tick. Sleep = 16 ticks. Magic = 1-4 ticks (Interpreter decides). Busy agents are skipped.

**Tick cycle:**
1. Randomize agent order for this tick
2. For each agent:
   a. If busy: decrement remaining ticks, apply per-tick effects (energy restore during sleep), skip
   b. Build perception prompt from world state
   c. Send to LLM, receive intended action
   d. Validate action (available here? target present? intent string provided for magic?)
   e. If valid: resolve (d20 for non-magic, Interpreter for magic)
   f. If invalid: wander (move to random adjacent location)
   g. Update world state, write memory entry
   h. Call GM Narrator — one LLM call for a vivid outcome sentence (skip for Cast Intent, use memory_entry)
   i. Log outcome
3. Apply passive need decay to all agents
4. Advance world clock
5. If state dump interval: write snapshot

### 5.3 Day/Night

`is_daytime: bool` flips at configurable tick. Night effects:
- Forage and Explore DCs increase by `night_dc_bonus` from config
- If Energy < 20 at night, perception prompt includes a strong sleep nudge

---

## 6. Agent Model

### 6.1 Identity

Defined in the soul seed file. Immutable across runs.

```rust
struct AgentIdentity {
    name: String,
    personality: String,
    backstory: String,
    magical_affinity: String,
    self_declaration: String,
}
```

### 6.2 Attributes

Five attributes, scored 1-10. Defined in soul seed. Static in MVP. **Must sum to 30** (enforced at seed parsing).

| Attribute     | Governs                                                     |
|---------------|-------------------------------------------------------------|
| **Vigor**     | Physical tasks, stamina, combat, labor                      |
| **Wit**       | Cleverness, crafting, problem-solving, cooking               |
| **Grace**     | Agility, stealth, fishing, delicate work                     |
| **Heart**     | Social interaction, persuasion, empathy, charm               |
| **Numen**     | Magical clarity — how faithfully intent manifests            |

**Modifier formula:** `modifier = attribute_score - 5`

### 6.3 Needs

Five needs, float 0.0-100.0. Reset to initial values each run.

| Need         | Decay/Tick | At < 20          | At < 10            | At < 5              |
|--------------|------------|-------------------|--------------------|---------------------|
| **Hunger**   | 1.5        | -2 all checks    | -4 all checks      | Perception: "starving" |
| **Energy**   | 1.2        | -2 physical checks| -4 physical checks | Forced sleep        |
| **Fun**      | 0.8        | Mood note only    | -2 all checks      | Perception: "deeply bored" |
| **Social**   | 0.6        | Mood note only    | -2 Heart checks    | Perception: "achingly lonely" |
| **Hygiene**  | 0.3        | Mood note only    | -2 Heart checks    | Others avoid Chat   |

### 6.4 Memory

Rolling buffer of last N events (default 20). Each entry is a one-line string with tick, day, time-of-day, and outcome description.

### 6.5 Agent State

```rust
struct Agent {
    id: AgentId,
    identity: AgentIdentity,
    attributes: Attributes,
    needs: Needs,
    location: LocationId,
    memory: VecDeque<String>,
    busy_ticks: u32,
    busy_action: Option<Action>,
}
```

---

## 7. Action System

### 7.1 Available Actions

| Action          | Location(s)        | Attribute | Satisfies           | Ticks | DC  |
|-----------------|--------------------|-----------|---------------------|-------|-----|
| **Eat**         | Tavern, Home       | —         | Hunger +30          | 1     | auto|
| **Cook**        | Tavern, Home       | Wit       | Hunger +45, Fun +10 | 1     | 12  |
| **Sleep**       | Home               | —         | Energy +6.25/tick   | 16    | auto|
| **Rest**        | Anywhere           | —         | Energy +15          | 1     | auto|
| **Forage**      | Forest             | Grace     | Hunger +20          | 1     | 10  |
| **Fish**        | River              | Grace     | Hunger +35, Fun +10 | 1     | 12  |
| **Exercise**    | Forest, Square     | Vigor     | Fun +15, Energy -10 | 1     | 10  |
| **Chat**        | Any (needs other)  | Heart     | Social +20, Fun +8  | 1     | 8   |
| **Bathe**       | River              | —         | Hygiene +50         | 1     | auto|
| **Explore**     | Forest             | Vigor     | Fun +20             | 1     | 12  |
| **Play**        | Square, Tavern     | —         | Fun +15             | 1     | auto|
| **Move**        | Any                | —         | —                   | 1     | auto|
| **Cast Intent** | Any                | Numen     | Varies              | 1-4   | auto|

### 7.2 Action Resolution (d20)

For non-magic, non-auto actions:

```
Roll = rand(1..=20)
Modifier = attribute_score - 5
NeedPenalty = sum of applicable penalties from needs thresholds
Total = Roll + Modifier + NeedPenalty
```

**Outcome tiers:**
- **Critical Fail** (natural 1): Half need restoration. Funny memory entry.
- **Fail** (Total < DC): No need restoration. Mild negative memory.
- **Success** (Total >= DC): Full need restoration as configured.
- **Critical Success** (natural 20): 1.5x need restoration. Bonus flavor.

### 7.3 Action Validation

Before resolution, validate:
1. Is the action available at the agent's current location?
2. If Chat: is another non-busy agent present at this location?
3. If Sleep: is agent at their assigned home?
4. If Cast Intent: does the response include an `intent` string?

If validation fails: agent wanders (Move to random adjacent location).

---

## 8. Magic System: Freeform Intent

> *"The world was spoken into being through the letters. When you speak your intent upon reality, reality listens — to every meaning your words carry, not merely the one you intended."*

### 8.1 Core Principle

No spell lists, no mana pools, no memorized incantations. An agent declares a **freeform intention upon reality**, and reality reshapes itself. **Spells always succeed**, but the world interprets the agent's words across *all their semantic meanings*. Ambiguity, metaphor, and double-meanings produce unexpected secondary effects.

The uncertainty is never "does it work?" — it's "what does it *really mean?*"

Inspired by Kabbalistic principles: the creative power of language (*Otiyot*), hidden meanings within letters, and the consequence of imprecise speech in a world woven from the Word.

### 8.2 Flow

1. Agent's LLM chooses "Cast Intent" and provides an `intent` string
2. World server sends intent to a **separate LLM call** — the **Intent Interpreter**
3. Interpreter analyzes all semantic dimensions, returns structured result
4. World server applies effects, writes outcome to agent's memory

### 8.3 Intent Interpreter Prompt

```
You are the Interpreter of Intent in the world of Nephara. A being has spoken
a desire upon reality, and reality must respond.

SPEAKER: {name}
NUMEN (magical clarity, 1-10): {numen_score}
LOCATION: {location_name}
NEARBY: {others_present}
WORLD STATE NOTES: {relevant context}

THE SPOKEN INTENT:
"{intent_text}"

Your task:
1. Identify the PRIMARY EFFECT — what the speaker most likely meant.
2. Analyze every word for SECONDARY MEANINGS — synonyms, metaphors, double
   meanings, emotional undertones, etymological echoes. List 2-3.
3. Based on Numen score, determine how the intent manifests:
   - Numen 1-3: Secondary meanings DOMINATE. Reality is creative and willful.
   - Numen 4-6: MIXED. Primary effect occurs, but secondary meanings also manifest.
   - Numen 7-9: CLEAN. Primary dominates. Secondary effects are subtle, poetic.
   - Numen 10: MASTERFUL. Almost exactly as meant. Secondary effects are beautiful.
4. Determine duration in ticks (1-4, more ambitious = longer).
5. Determine need changes for the caster.

CRITICAL: The spell ALWAYS SUCCEEDS. Never say "nothing happens." Every intent
produces something interesting. Wild misinterpretations should feel like stories,
not punishment.

Respond with ONLY a JSON object:
{
  "primary_effect": "What happens as intended",
  "interpretations": ["secondary meaning 1", "secondary meaning 2"],
  "secondary_effect": "What else happens due to the words' other meanings",
  "duration_ticks": 1,
  "need_changes": {"fun": 10, "energy": -8},
  "memory_entry": "One-line summary for the caster's memory"
}
```

### 8.4 Magic Boundaries (Soft Guardrails)

Enforced in the Interpreter prompt:
- **No direct harm** — "I want X to die" manifests as adjacent effects (chill, vision, melancholy), not literal death
- **No world-breaking** — "destroy the village" causes tremors, unease, cracks, not destruction
- **Effects are local and temporary** unless Numen is very high
- **Caster always pays energy cost** (configured in TOML)

### 8.5 MVP Fallback

If the small local model can't do good semantic analysis:
1. Use Opus via API for interpretation calls only (higher quality, costs money)
2. Template-based reinterpretation system (deterministic, less magical)

Try freeform first.

---

## 9. LLM Integration

### 9.1 Agent Decision Prompt

```
You are {name}. {personality}

{backstory}

CURRENT STATE:
- Location: {location_name} — {location_description}
- Time: Day {day}, {time_of_day} (Tick {tick_number})
- Hunger: {hunger}/100 | Energy: {energy}/100 | Fun: {fun}/100
- Social: {social}/100 | Hygiene: {hygiene}/100
{need_warnings}

NEARBY:
{occupant_list_or "You are alone."}

RECENT MEMORY:
{last N memory entries, newest first}

AVAILABLE ACTIONS:
{numbered list filtered by location + validation}
(You may also Cast Intent — speak a desire upon reality. It will manifest,
though perhaps not as you expect.)

Choose ONE action. Respond with ONLY a JSON object:
{"action": "action_name", "target": "optional_target", "intent": "if casting, your spoken desire", "reason": "brief reason"}
```

`{need_warnings}` generated dynamically:
- Hunger < 20: "You are very hungry. Your body aches for food."
- Energy < 10: "You are exhausted. You can barely keep your eyes open."
- Social < 10: "You feel achingly lonely."
- Etc.

### 9.2 Response Parsing

Cascading fallbacks (simulation must never crash):

1. Parse as JSON directly
2. Extract JSON from markdown code fences
3. Regex for `"action"\s*:\s*"(\w+)"` to get action name
4. Default: Move to random adjacent location

Log parse failures at WARN level.

### 9.3 Chat Action

MVP: single LLM call with both agents' personalities and recent memories, returns a one-sentence conversation summary. Both agents get the summary in their memory.

### 9.5 World GM Agent (Narrator)

After every d20 resolution, the World makes a short second LLM call to generate one vivid narrative sentence describing what happened.

**Prompt:**
```
You are the Narrator of Nephara.
{name} attempted to {action_display} at {location_name}.
{Alone. | {others} watched.}
Outcome: {tier_label}.

Write ONE vivid sentence (15-25 words). Pure story — no numbers, no dice.
```

**Rules:**
- Skip GM call for Cast Intent — use the Interpreter's `memory_entry` field instead (it's already narrative)
- Max tokens: 80 (tight budget — one sentence only)
- Fallback: `auto_narrative()` template if LLM returns empty or errors
- Seed: `main_seed + llm_call_counter` (consistent with Interpreter call pattern)

**Output placement:** The vivid sentence is the `outcome_line` of the `TickEntry`. Need changes are shown in the needs footer only, not appended inline (except Cast Intent which shows `[changes, N ticks]`).

### 9.4 Ollama HTTP

```
POST {config.llm.ollama_url}/api/generate
{
    "model": "{config.llm.model}",
    "prompt": "...",
    "stream": false,
    "options": {
        "temperature": {config.llm.temperature},
        "num_predict": {max_tokens}
    }
}
```

---

## 10. Persistence & Soul Seeds

### 10.1 Soul Seed File Format

Markdown with YAML frontmatter. Rust parses frontmatter for structured data, body for identity text.

```markdown
---
name: "[self-chosen during summoning]"
vigor: 6
wit: 8
grace: 4
heart: 7
numen: 5
summoned: "2026-03-05"
summoner: "Archwizard"
---

# [Name]

## Personality
[Free-text paragraph]

## Backstory
[Free-text paragraph]

## Magical Affinity
[Free-text paragraph about relationship to intent-casting]

## Self-Declaration
[First-person statement — the entity's own words about what it is]
```

**Constraint:** Attributes must sum to 30.

### 10.2 Journal

After each run, append to `souls/[name].journal.md`:

```markdown
## Run {run_id} — {date} — {days} days ({ticks} ticks)

- [Notable event 1]
- [Notable event 2]
- [Notable event 3]
```

Notable = critical successes/failures, magic casts, first interactions, needs below 10.

### 10.3 Persistence Rules

| Persists                         | Resets Each Run                  |
|----------------------------------|----------------------------------|
| Name, personality, backstory     | Needs (start at config initial)  |
| Attributes                       | Memory buffer (empty)            |
| Journal entries                  | Location (start at own Home)     |
| Magical affinity                 | Busy state (free)                |

---

## 11. Output & Observability

### 11.1 Tick Log

Printed to stdout and saved to `runs/{timestamp}/tick_log.txt`:

```
=== TICK 14 | Day 1 | Morning ===
  [Rowan     ] @ Tavern           | Cook | Wit 14 vs DC 12 | Success
             > She hums to herself as steam rises from the pot,
               a smell that reaches the square before she does.
  [Elara     ] @ Forest           | Forage | Grace 11 vs DC 10 | Success
             > Fingers move through the undergrowth with practiced
               patience. She finds something worth keeping.
  [Thane     ] @ River            | Cast Intent: "let the water remember"
             > The river slows imperceptibly. Something stirs beneath.
               [Energy -8, Fun +12, 2 ticks]

  Needs: Rowan [H:62 E:78 F:48 S:32 Y:71] | Elara [H:45 E:65 F:58 S:55 Y:82] | Thane [H:70 E:52 F:68 S:40 Y:60]
```

Key format rules:
- `> narrative` may wrap to 2+ lines (indent continuation with 2 extra spaces beyond `> `)
- Cast Intent narrative comes from Interpreter's `memory_entry` field
- Need changes NOT shown inline for regular actions (needs footer handles that)
- `[changes, N ticks]` shown only for Cast Intent, on a continuation line

### 11.2 State Dump

JSON file every N ticks. Contains full world state.

### 11.3 Run Summary

At end of simulation: total ticks, notable events, final need states, total magic casts. Append journal entries for each agent.

---

## 12. MVP Milestones

### M0: Foundation (est. 1-2 sessions)
- [ ] `flake.nix` with Rust toolchain
- [ ] `cargo init`, add dependencies from 3.3
- [ ] World state structs with serde
- [ ] TOML config loading
- [ ] 5 locations with adjacency graph
- [ ] Soul seed parser (frontmatter + markdown body)
- [ ] LLM backend trait + MockBackend

### M0.5: The Summoning (1 session, done by Archwizard)
- [ ] Craft `rituals/summoning.md`
- [ ] Run 3 invocations against Opus 4.6
- [ ] Review seeds, verify attributes sum to 30
- [ ] Commit to `souls/`

### M1: Game Loop (est. 2-3 sessions)
- [ ] Tick loop with MockBackend
- [ ] d20 action resolution
- [ ] Need decay and satisfaction per config
- [ ] Multi-tick actions (busy state)
- [ ] Action validation
- [ ] Memory buffer
- [ ] Tick log (stdout + file)
- [ ] State dump to JSON
- [ ] 144-tick test run: agents survive and act sanely

### M2: LLM Integration (est. 2-3 sessions)
- [ ] OllamaBackend implementation
- [ ] Perception prompt construction
- [ ] Response parsing with fallbacks
- [ ] Chat action summary generation
- [ ] LLM-driven agent decisions
- [ ] Test with Gemma 3 4B

### M2.5: Magic (est. 1-2 sessions)
- [ ] Cast Intent action flow
- [ ] Intent Interpreter prompt + separate LLM call
- [ ] Parse Interpreter response, apply effects
- [ ] Numen-scaled clarity
- [ ] Magic in memory and tick log
- [ ] Test: each Founding entity casts at least once

### M2.7: World GM Agent (est. 1 session)
- [ ] `build_gm_prompt()` in world.rs
- [ ] GM Narrator call after each d20 resolution (skip for Cast Intent)
- [ ] MockBackend: detect `"Narrator of Nephara"` prompt, return vivid mock narrative
- [ ] MockBackend: detect `"primary_effect"` prompt, return valid InterpretedIntent JSON
- [ ] MockBackend: detect `"brief conversation"` prompt, return chat summary (already done)
- [ ] Log wrapping: `TickEntry::format()` wraps at ~70 chars, continuation indent
- [ ] CastIntent format: `memory_entry` as narrative + `[changes, N ticks]` on continuation

### M3: Polish & Observe (est. 1-2 sessions)
- [ ] Multi-day simulation runs
- [ ] Tune config values
- [ ] Journal generation
- [ ] Run summary
- [ ] Document findings, decide post-MVP priorities

---

## 13. Post-MVP Ideas

- Inventory and trading
- Crafting system
- Relationship/affinity scores
- Random world events (storms, festivals, strangers)
- Deeper memory (reflection, summarization, long-term goals)
- **TUI viewer (`ratatui`)** — three-panel layout:
  - Left panel: ASCII village map with agent positions updated each tick
  - Center panel: scrolling tick log (the current stdout output)
  - Right panel: agent status cards (name, location, needs bars, last memory entry)
  - Keybindings: `q` quit, `space` pause/resume, `+`/`-` tick speed, `1`/`2`/`3` focus agent
- More agents (5-10)
- Attribute growth through use
- Day/night environmental changes
- Human player agent via CLI
- Persistent world effects from magic
- Inter-agent magic targeting
- Opus API as Interpreter backend

---

## 14. Determinism & Replay

The simulation is **fully deterministic given a fixed seed**.

### 14.1 Mechanism

- A `u64` seed is provided via `--seed <N>` CLI flag. If omitted, a random seed is generated and logged.
- All random operations use `rand::rngs::StdRng::seed_from_u64(seed)`:
  - Agent ordering per tick (`shuffle`)
  - d20 dice rolls
  - MockBackend action selection
- LLM calls pass `seed` and `temperature: 0` to Ollama's generate options for deterministic outputs.
- Each successive LLM call derives its seed as `main_seed + call_counter`, so calls are distinct.

### 14.2 Replay

```
cargo run -- --seed <N> --ticks <T> --llm <BACKEND>
```

The seed is printed at startup, embedded in the `runs/` directory name, and saved in every state dump JSON.

### 14.3 Caveats

- LLM determinism requires the **same model version** on the same hardware. A model update will change outputs even with the same seed.
- `--llm mock` is fully deterministic regardless of hardware and is preferred for testing.

---

## 15. Open Questions

1. **Interpreter model quality:** Can Gemma 3 4B do semantic analysis well enough? Test early.
2. **Magic boundary enforcement:** Soft guardrails in prompt — monitor for creative workarounds.
3. **Simultaneous action conflicts:** Randomized agent order per tick. First-mover advantage is intentional.
4. **Flavor text:** Template-based for MVP, LLM-generated as stretch.
5. **Summoning prompt depth:** Balance cosmological context with room for self-definition.

---

## 16. Key References

- **Generative Agents: Interactive Simulacra of Human Behavior** — Park et al., 2023 (arXiv)
- **D&D 5e SRD, Chapter 7: Using Ability Scores** — free at 5esrd.com
- **Designing Virtual Worlds** — Richard Bartle
- **Growing Artificial Societies (Sugarscape)** — Epstein & Axtell
- **Characteristics of Games** — Elias, Garfield & Gutschera
- **Rules of Play** — Salen & Zimmerman
- **Sefer Yetzirah** — creation through letters
- **Zohar (selections)** — concealed meanings within language
