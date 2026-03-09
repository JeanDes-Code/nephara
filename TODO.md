# Nephara — Post-MVP Improvements

## 1. Long-Term Memory with Summarization
**Status:** Done
**Priority:** Highest — smallest change, biggest payoff for emergent behavior

Add two-tier memory: keep the 20-event short-term rolling buffer, but at end of each day have the LLM compress the day's events into a 2-3 sentence summary stored in a long-term memory vec. Include the last few day summaries in the perception prompt. Gives agents continuity across days without blowing up context length.

- [x] Add `long_term_memory: Vec<String>` to Agent
- [x] Add end-of-day summarization LLM call
- [x] Include long-term summaries in perception prompt
- [x] Add config knobs (max summaries kept, summary token budget)
- [x] Serialize long-term memory in state dumps

## 2. Agent Relationships & Social Memory
**Status:** Not started
**Priority:** High — builds on long-term memory

Per-agent-pair affinity score (-100 to +100) that shifts based on interactions (chatting, praising, casting magic nearby). Feed into decision prompt so agents develop preferences and social dynamics.

- [ ] Add affinity map `HashMap<(AgentId, AgentId), f32>` to World
- [ ] Update affinity after social actions (Chat, Praise, shared location events)
- [ ] Include relationship context in perception prompt
- [ ] Add config knobs (affinity change rates, decay over time)
- [ ] Serialize in state dumps

## 3. World Events
**Status:** Not started
**Priority:** Medium — makes the world feel alive

Random or scheduled events (rainstorms, festivals, magical anomalies, seasonal changes) that modify action DCs, need restoration, or agent perception. Defined in world.toml.

- [ ] Define WorldEvent struct (name, description, duration, effects, trigger conditions)
- [ ] Add event definitions to world.toml
- [ ] Event scheduler (probability per tick, time-of-day/season triggers)
- [ ] Inject active events into agent perception prompts
- [ ] Modify action DCs and restorations based on active events
- [ ] Display events in TUI and tick log

## 4. Persistent Magic Effects
**Status:** Not started
**Priority:** Medium — makes magic consequential

Spells can leave persistent effects on locations or other agents, not just the caster. Enchanted river yields more fish, blessed hearth restores more energy.

- [ ] Extend Intent Interpreter to return target (location/agent/self) and effect type
- [ ] Add active effects list to Location struct with tick-based expiry
- [ ] Add active effects list to Agent struct (buffs/debuffs)
- [ ] Factor active effects into action resolution (DC modifiers, restoration bonuses)
- [ ] Display active effects in TUI and perception prompts
- [ ] Add config knobs (max effect duration, Numen scaling)

## 5. Human Observer/Player Mode
**Status:** Not started
**Priority:** Lower — most complex, most rewarding

Human player agent in TUI who can issue one action per tick via keyboard. Pick from available actions or type freeform magic intent. Results narrated alongside AI agents.

- [ ] Add human player agent type (skip LLM decision call)
- [ ] TUI action selection UI (list of available actions + freeform input for magic)
- [ ] Integrate human actions into tick loop (pause for input)
- [ ] Narrate human actions same as AI agents
- [ ] CLI flag to enable player mode (--player)
