use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::Rng;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{info, warn};

use crate::action::{self, Action, OutcomeTier, Resolution};
use crate::agent::{Agent, LocationId, NeedChanges};
use crate::config::Config;
use crate::llm::LlmBackend;
use crate::log::{self as runlog, RunLog, TickEntry};
use crate::magic;
use crate::soul::SoulSeed;

// ---------------------------------------------------------------------------
// Location
// ---------------------------------------------------------------------------

pub struct Location {
    pub id:          LocationId,
    pub name:        &'static str,
    pub description: &'static str,
    pub adjacent:    Vec<LocationId>,
    /// Which action names are available here (lowercase).
    pub affordances: Vec<&'static str>,
}

// ---------------------------------------------------------------------------
// Tick log (returned from World::tick)
// ---------------------------------------------------------------------------

pub struct TickResult {
    pub tick:        u32,
    pub day:         u32,
    pub time_of_day: &'static str,
    pub entries:     Vec<TickEntry>,
}

// ---------------------------------------------------------------------------
// World
// ---------------------------------------------------------------------------

pub struct World {
    pub tick_num:       u32,
    pub agents:         Vec<Agent>,
    pub seed:           u64,
    pub config:         Config,
    pub run_log:        RunLog,
    pub notable_events: Vec<(usize, String)>, // (agent_id, event)
    pub magic_count:    u32,
    locations:          HashMap<String, Location>,
    rng:                StdRng,
    llm:                Arc<dyn LlmBackend>,
    llm_call_counter:   u64, // increments per LLM call — used as per-call seed offset
}

impl World {
    // -----------------------------------------------------------------------
    // Construction
    // -----------------------------------------------------------------------

    pub fn new(
        seeds:  Vec<SoulSeed>,
        config: Config,
        seed:   u64,
        rng:    StdRng,
        llm:    Arc<dyn LlmBackend>,
        run_log: RunLog,
    ) -> Self {
        let agents    = seeds.iter().enumerate()
            .map(|(i, s)| Agent::from_soul(i, s, &config))
            .collect();
        let locations = build_locations();
        World {
            tick_num: 0,
            agents,
            seed,
            config,
            run_log,
            notable_events: Vec::new(),
            magic_count: 0,
            locations,
            rng,
            llm,
            llm_call_counter: 0,
        }
    }

    // -----------------------------------------------------------------------
    // Tick
    // -----------------------------------------------------------------------

    pub async fn tick(&mut self) -> Result<TickResult, Box<dyn std::error::Error + Send + Sync>> {
        let tick        = self.tick_num;
        let tpd         = self.config.time.ticks_per_day;
        let day         = tick / tpd + 1;
        let tick_in_day = tick % tpd;
        let is_night    = tick_in_day >= self.config.time.night_start_tick;
        let tod         = runlog::time_of_day(tick_in_day, self.config.time.night_start_tick);

        // Randomise agent order each tick
        let mut order: Vec<usize> = (0..self.agents.len()).collect();
        order.shuffle(&mut self.rng);

        let mut entries = Vec::new();

        for &idx in &order {
            let entry = self.process_agent(idx, tick, day, is_night, tod).await?;
            entries.push(entry);
        }

        // Passive need decay
        for agent in &mut self.agents {
            agent.needs.apply_decay(&self.config.needs.decay_per_tick);
        }

        self.tick_num += 1;

        Ok(TickResult { tick, day, time_of_day: tod, entries })
    }

    // -----------------------------------------------------------------------
    // Process one agent for the current tick
    // -----------------------------------------------------------------------

    async fn process_agent(
        &mut self,
        idx:     usize,
        tick:    u32,
        day:     u32,
        is_night: bool,
        tod:     &str,
    ) -> Result<TickEntry, Box<dyn std::error::Error + Send + Sync>> {
        // --- Busy tick ---
        if self.agents[idx].is_busy() {
            // Apply sleep energy restoration
            if let Some(energy) = self.agents[idx].sleep_energy_tick {
                self.agents[idx].needs.energy += energy;
                self.agents[idx].needs.clamp();
            }
            self.agents[idx].busy_ticks -= 1;
            let ticks_left = self.agents[idx].busy_ticks;
            let loc_name   = self.location_name(&self.agents[idx].location).to_string();
            return Ok(TickEntry {
                agent_name:  self.agents[idx].name().to_string(),
                location:    loc_name,
                action_line: format!("(busy — {} tick{} remaining)", ticks_left, if ticks_left == 1 { "" } else { "s" }),
                outcome_line: String::new(),
            });
        }

        // --- Forced sleep if energy < forced_action threshold ---
        let action = if self.agents[idx].needs.energy < self.config.needs.thresholds.forced_action
            && matches!(self.agents[idx].location, LocationId::Home(_))
        {
            Action::Sleep
        } else if self.agents[idx].needs.energy < self.config.needs.thresholds.forced_action {
            // Move home first
            let home_name = self.location_name(&self.agents[idx].home).to_string();
            Action::Move { destination: home_name }
        } else {
            // Build prompt and ask LLM
            let prompt = self.build_prompt(idx, tick, day, is_night, tod);
            let call_seed = Some(self.seed.wrapping_add(self.llm_call_counter));
            self.llm_call_counter += 1;
            let llm = Arc::clone(&self.llm);
            let raw = llm
                .generate(&prompt, self.config.llm.max_tokens, call_seed)
                .await
                .unwrap_or_else(|e| {
                    warn!("LLM error for {}: {}", self.agents[idx].name(), e);
                    String::new()
                });
            action::parse_response(&raw)
        };

        // --- Validate and resolve ---
        let action    = self.validate(idx, action);
        let loc_name  = self.location_name(&self.agents[idx].location).to_string();
        let entry     = self.resolve_and_apply(idx, action, &loc_name, tick, day, tod, is_night).await?;

        Ok(entry)
    }

    // -----------------------------------------------------------------------
    // Validate action — returns the action unchanged or Wander
    // -----------------------------------------------------------------------

    fn validate(&self, idx: usize, action: Action) -> Action {
        let loc = &self.agents[idx].location;

        match action {
            Action::Eat     if !self.location_allows(loc, "eat")     => self.wander_action(idx),
            Action::Cook    if !self.location_allows(loc, "cook")    => self.wander_action(idx),
            Action::Sleep   if *loc != self.agents[idx].home         => self.wander_action(idx),
            Action::Forage  if !self.location_allows(loc, "forage")  => self.wander_action(idx),
            Action::Fish    if !self.location_allows(loc, "fish")    => self.wander_action(idx),
            Action::Exercise if !self.location_allows(loc, "exercise") => self.wander_action(idx),
            Action::Bathe   if !self.location_allows(loc, "bathe")   => self.wander_action(idx),
            Action::Explore if !self.location_allows(loc, "explore") => self.wander_action(idx),
            Action::Wander  => self.wander_action(idx),

            Action::Chat { target_name } => {
                // Check if named target is present and not busy
                let target_ok = self.agents.iter().enumerate().any(|(i, a)| {
                    i != idx
                        && a.name().eq_ignore_ascii_case(&target_name)
                        && a.location == *loc
                        && !a.is_busy()
                });
                if target_ok {
                    return Action::Chat { target_name };
                }
                // Try any available chat partner
                let partner_name = self.agents.iter()
                    .find(|a| a.id != idx && a.location == *loc && !a.is_busy())
                    .map(|a| a.name().to_string());
                match partner_name {
                    Some(name) => Action::Chat { target_name: name },
                    None       => self.wander_action(idx),
                }
            }

            Action::Move { destination } => {
                let dest_id = self.parse_location(&destination);
                match dest_id {
                    Some(dest) if dest == *loc || self.is_adjacent(loc, &dest) => {
                        Action::Move { destination: self.location_name(&dest).to_string() }
                    }
                    _ => self.wander_action(idx),
                }
            }

            other => other,
        }
    }

    fn wander_action(&self, idx: usize) -> Action {
        let loc = &self.agents[idx].location;
        if let Some(location) = self.locations.get(&self.location_key(loc)) {
            if !location.adjacent.is_empty() {
                // Pick a deterministic-ish adjacent location using current tick + agent id
                let pick = (self.tick_num as usize + idx) % location.adjacent.len();
                let dest  = &location.adjacent[pick];
                return Action::Move {
                    destination: self.location_name(dest).to_string(),
                };
            }
        }
        Action::Rest
    }

    // -----------------------------------------------------------------------
    // Resolve and apply
    // -----------------------------------------------------------------------

    async fn resolve_and_apply(
        &mut self,
        idx:      usize,
        action:   Action,
        loc_name: &str,
        tick:     u32,
        day:      u32,
        tod:      &str,
        is_night:  bool,
    ) -> Result<TickEntry, Box<dyn std::error::Error + Send + Sync>> {
        match action {
            // ---- Move ----
            Action::Move { destination } => {
                let dest_id   = self.parse_location(&destination)
                    .unwrap_or(LocationId::VillageSquare);
                let dest_name = self.location_name(&dest_id).to_string();
                self.agents[idx].location = dest_id;
                let mem = format!("Tick {tick} | Day {day} | {tod} | Moved to {dest_name}");
                let buf = self.config.memory.buffer_size;
                self.agents[idx].push_memory(mem, buf);
                Ok(TickEntry {
                    agent_name:   self.agents[idx].name().to_string(),
                    location:     loc_name.to_string(),
                    action_line:  format!("Move > {}", dest_name),
                    outcome_line: format!("{} walks to {}.", self.agents[idx].name(), dest_name),
                })
            }

            // ---- Chat ----
            Action::Chat { target_name } => {
                self.resolve_chat(idx, &target_name, loc_name, tick, day, tod, is_night).await
            }

            // ---- Cast Intent ----
            Action::CastIntent { intent } => {
                self.resolve_cast_intent(idx, &intent, loc_name, tick, day, tod).await
            }

            // ---- Sleep ----
            Action::Sleep => {
                let duration     = self.config.actions.sleep.duration_ticks.unwrap_or(16);
                let energy_ptick = self.config.actions.sleep.energy_restore_per_tick.unwrap_or(6.25);
                self.agents[idx].busy_ticks        = duration - 1;
                self.agents[idx].sleep_energy_tick = Some(energy_ptick);
                self.agents[idx].needs.energy     += energy_ptick;
                self.agents[idx].needs.clamp();
                let mem = format!("Tick {tick} | Day {day} | {tod} | Fell asleep");
                let buf = self.config.memory.buffer_size;
                self.agents[idx].push_memory(mem, buf);
                Ok(TickEntry {
                    agent_name:   self.agents[idx].name().to_string(),
                    location:     loc_name.to_string(),
                    action_line:  "Sleep".to_string(),
                    outcome_line: format!("{} falls into a deep sleep.", self.agents[idx].name()),
                })
            }

            // ---- Standard d20 resolution ----
            action => {
                let res = {
                    // Borrow different fields simultaneously (fields are distinct)
                    let attrs  = &self.agents[idx].attributes;
                    let needs  = &self.agents[idx].needs;
                    let config = &self.config;
                    action::resolve(&action, attrs, needs, config, is_night, &mut self.rng)
                };

                if res.duration > 1 {
                    self.agents[idx].busy_ticks        = res.duration - 1;
                    self.agents[idx].sleep_energy_tick = None;
                }
                self.agents[idx].needs.apply(&res.need_changes);

                // Collect context for GM prompt
                let nearby: Vec<String> = self.agents.iter()
                    .filter(|a| a.id != idx && a.location == self.agents[idx].location)
                    .map(|a| a.name().to_string())
                    .collect();
                let agent_name_str = self.agents[idx].name().to_string();
                let gm_prompt = Self::build_gm_prompt(
                    &agent_name_str, &res.action.display(), &res.tier, loc_name, &nearby,
                );
                let call_seed = Some(self.seed.wrapping_add(self.llm_call_counter));
                self.llm_call_counter += 1;
                let llm = Arc::clone(&self.llm);
                let narrative = match llm.generate(&gm_prompt, 80, call_seed).await {
                    Ok(n) if !n.trim().is_empty() => n.trim().to_string(),
                    _ => self.narrative_for(&res, idx),
                };

                let check_line   = res.check_line();
                let action_line  = if check_line.is_empty() {
                    res.action.display()
                } else {
                    format!("{} | {}", res.action.display(), check_line)
                };

                if res.tier == OutcomeTier::CriticalSuccess {
                    let ev = format!("Day {day}: {} got a critical success on {}",
                        self.agents[idx].name(), res.action.name());
                    self.notable_events.push((idx, ev));
                }
                if res.tier == OutcomeTier::CriticalFail {
                    let ev = format!("Day {day}: {} critically failed at {}",
                        self.agents[idx].name(), res.action.name());
                    self.notable_events.push((idx, ev));
                }

                let needs_note = res.need_changes.describe();
                let mem = format!("Tick {tick} | Day {day} | {tod} | {} — {} [{}]",
                    res.action.name(), res.tier.label(), needs_note);
                let buf = self.config.memory.buffer_size;
                self.agents[idx].push_memory(mem, buf);

                Ok(TickEntry {
                    agent_name:  self.agents[idx].name().to_string(),
                    location:    loc_name.to_string(),
                    action_line,
                    outcome_line: narrative,
                })
            }
        }
    }

    // -----------------------------------------------------------------------
    // Chat resolution
    // -----------------------------------------------------------------------

    async fn resolve_chat(
        &mut self,
        idx:       usize,
        target:    &str,
        loc_name:  &str,
        tick:      u32,
        day:       u32,
        tod:       &str,
        is_night:   bool,
    ) -> Result<TickEntry, Box<dyn std::error::Error + Send + Sync>> {
        let target_idx = self.agents.iter().position(|a| a.name().eq_ignore_ascii_case(target));
        let target_idx = match target_idx {
            Some(i) => i,
            None    => {
                return Ok(TickEntry {
                    agent_name:  self.agents[idx].name().to_string(),
                    location:    loc_name.to_string(),
                    action_line: format!("Chat with {}", target),
                    outcome_line: format!("{} looks around for {} but finds no one.", self.agents[idx].name(), target),
                });
            }
        };

        let chat_prompt = self.build_chat_prompt(idx, target_idx);
        let call_seed   = Some(self.seed.wrapping_add(self.llm_call_counter));
        self.llm_call_counter += 1;
        let llm         = Arc::clone(&self.llm);
        let summary     = llm
            .generate(&chat_prompt, 80, call_seed)
            .await
            .unwrap_or_else(|_| {
                format!("{} and {} exchange a few words.", self.agents[idx].name(), self.agents[target_idx].name())
            });
        let summary = summary.trim().trim_matches('"').to_string();

        // Roll for initiating agent
        let res = {
            let agent = &self.agents[idx];
            action::resolve(
                &Action::Chat { target_name: target.to_string() },
                &agent.attributes, &agent.needs, &self.config, is_night, &mut self.rng,
            )
        };

        let changes  = res.need_changes.clone();
        let buf      = self.config.memory.buffer_size;
        let mem_a    = format!("Tick {tick} | Day {day} | {tod} | Chat with {} — \"{}\". [{}]",
            self.agents[target_idx].name(), &summary[..summary.len().min(80)], changes.describe());
        let mem_b    = format!("Tick {tick} | Day {day} | {tod} | Chat with {} — \"{}\". [{}]",
            self.agents[idx].name(), &summary[..summary.len().min(80)], changes.describe());

        // Apply to initiating agent
        self.agents[idx].needs.apply(&changes);
        self.agents[idx].push_memory(mem_a, buf);

        // Apply to target agent (same need changes)
        self.agents[target_idx].needs.apply(&changes);
        self.agents[target_idx].push_memory(mem_b, buf);

        let check_line = res.check_line();
        Ok(TickEntry {
            agent_name:  self.agents[idx].name().to_string(),
            location:    loc_name.to_string(),
            action_line: format!("Chat with {} | {}", self.agents[target_idx].name(), check_line),
            outcome_line: format!("{} [{}]", summary, changes.describe()),
        })
    }

    // -----------------------------------------------------------------------
    // Cast Intent resolution
    // -----------------------------------------------------------------------

    async fn resolve_cast_intent(
        &mut self,
        idx:      usize,
        intent:   &str,
        loc_name: &str,
        tick:     u32,
        day:      u32,
        tod:      &str,
    ) -> Result<TickEntry, Box<dyn std::error::Error + Send + Sync>> {
        let others: Vec<String> = self.agents.iter()
            .filter(|a| a.id != idx && a.location == self.agents[idx].location)
            .map(|a| a.name().to_string())
            .collect();

        let prompt   = magic::build_interpreter_prompt(
            &self.agents[idx], intent, loc_name, &others, &self.config,
        );
        let call_seed = Some(self.seed.wrapping_add(self.llm_call_counter));
        self.llm_call_counter += 1;
        let llm       = Arc::clone(&self.llm);
        let raw       = llm
            .generate(&prompt, self.config.llm.interpreter_max_tokens, call_seed)
            .await
            .unwrap_or_default();

        let energy_drain = self.config.actions.cast_intent.energy_drain.unwrap_or(8.0);
        let interpreted  = magic::parse_interpreter_response(&raw)
            .unwrap_or_else(|| magic::fallback_intent(intent, energy_drain));

        let duration    = interpreted.clamped_duration(&self.config);
        let need_changes = interpreted.to_need_changes(&self.config);

        // Set busy if duration > 1
        if duration > 1 {
            self.agents[idx].busy_ticks        = duration - 1;
            self.agents[idx].sleep_energy_tick = None;
        }

        self.agents[idx].needs.apply(&need_changes);
        self.magic_count += 1;

        let ev = format!(
            "Day {day}: {} cast intent: \"{}\" → {}",
            self.agents[idx].name(), intent, &interpreted.primary_effect[..interpreted.primary_effect.len().min(60)]
        );
        self.notable_events.push((idx, ev));

        let mem = format!(
            "Tick {tick} | Day {day} | {tod} | {}",
            interpreted.memory_entry
        );
        let buf = self.config.memory.buffer_size;
        self.agents[idx].push_memory(mem, buf);

        let meta = format!(
            "[{}, {} tick{}]",
            need_changes.describe(),
            duration,
            if duration == 1 { "" } else { "s" },
        );
        let full_outcome = if interpreted.secondary_effect.is_empty() {
            format!("{}\n{}", interpreted.memory_entry, meta)
        } else {
            format!(
                "{}\n{}\n(secondary: {})",
                interpreted.memory_entry, meta, interpreted.secondary_effect
            )
        };

        Ok(TickEntry {
            agent_name:  self.agents[idx].name().to_string(),
            location:    loc_name.to_string(),
            action_line: format!("Cast Intent: \"{}\"", intent),
            outcome_line: full_outcome,
        })
    }

    // -----------------------------------------------------------------------
    // Prompt builders
    // -----------------------------------------------------------------------

    fn build_prompt(&self, idx: usize, tick: u32, day: u32, is_night: bool, tod: &str) -> String {
        let agent     = &self.agents[idx];
        let loc_name  = self.location_name(&agent.location);
        let loc_desc  = self.location_desc(&agent.location);

        // Others at same location
        let nearby: Vec<String> = self.agents.iter()
            .filter(|a| a.id != idx && a.location == agent.location)
            .map(|a| {
                if a.is_busy() {
                    format!("{} (busy)", a.name())
                } else {
                    a.name().to_string()
                }
            })
            .collect();
        let nearby_str = if nearby.is_empty() {
            "You are alone.".to_string()
        } else {
            nearby.join(", ")
        };

        // Recent memory (newest first)
        let memory_str: Vec<String> = agent.memory.iter().take(5).cloned().collect();
        let memory_block = if memory_str.is_empty() {
            "  (no memories yet)".to_string()
        } else {
            memory_str.iter().map(|m| format!("  - {}", m)).collect::<Vec<_>>().join("\n")
        };

        // Need warnings
        let warnings     = agent.need_warnings(&self.config);
        let warnings_str = if warnings.is_empty() {
            String::new()
        } else {
            format!("\nWARNINGS:\n{}", warnings.iter().map(|w| format!("  ! {}", w)).collect::<Vec<_>>().join("\n"))
        };

        // Available actions filtered by location
        let available = self.available_actions(idx, is_night);
        let actions_str = available.iter().enumerate()
            .map(|(i, a)| format!("  {}. {}", i + 1, a))
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            r#"You are {name}. {personality}

{backstory}

CURRENT STATE:
- Location: {loc_name} — {loc_desc}
- Time: Day {day}, {tod} (Tick {tick}){night_note}
- Hunger: {hunger:.0}/100 | Energy: {energy:.0}/100 | Fun: {fun:.0}/100
- Social: {social:.0}/100 | Hygiene: {hygiene:.0}/100
{warnings}

NEARBY:
{nearby}

RECENT MEMORY (newest first):
{memory}

AVAILABLE ACTIONS:
{actions}
(You may also Cast Intent — speak a desire upon reality. It will manifest,
though perhaps not as you expect.)

Choose ONE action. Respond with ONLY a JSON object:
{{"action": "action_name", "target": "optional_target_name", "intent": "if casting, your spoken desire", "reason": "brief reason"}}"#,
            name        = agent.identity.name,
            personality = agent.identity.personality,
            backstory   = agent.identity.backstory,
            loc_name    = loc_name,
            loc_desc    = loc_desc,
            day         = day,
            tod         = tod,
            tick        = tick,
            night_note  = if is_night { " [NIGHT]" } else { "" },
            hunger      = agent.needs.hunger,
            energy      = agent.needs.energy,
            fun         = agent.needs.fun,
            social      = agent.needs.social,
            hygiene     = agent.needs.hygiene,
            warnings    = warnings_str,
            nearby      = nearby_str,
            memory      = memory_block,
            actions     = actions_str,
        )
    }

    fn build_gm_prompt(
        agent_name:  &str,
        action_display: &str,
        tier:        &crate::action::OutcomeTier,
        loc_name:    &str,
        nearby:      &[String],
    ) -> String {
        let context = if nearby.is_empty() {
            "Alone.".to_string()
        } else {
            format!("{} watched.", nearby.join(", "))
        };
        format!(
            "You are the Narrator of Nephara.\n\
             {agent_name} attempted to {action_display} at {loc_name}.\n\
             {context}\n\
             Outcome: {tier}.\n\n\
             Write ONE vivid sentence (15-25 words). Pure story — no numbers, no dice.",
            agent_name     = agent_name,
            action_display = action_display,
            loc_name       = loc_name,
            context        = context,
            tier           = tier.label(),
        )
    }

    fn build_chat_prompt(&self, a_idx: usize, b_idx: usize) -> String {
        let a = &self.agents[a_idx];
        let b = &self.agents[b_idx];
        let a_mem = a.memory.iter().next().cloned().unwrap_or_default();
        let b_mem = b.memory.iter().next().cloned().unwrap_or_default();
        format!(
            r#"Two villagers in Nephara are having a brief conversation.

{a_name}: {a_personality}
  Recent memory: {a_mem}

{b_name}: {b_personality}
  Recent memory: {b_mem}

Write ONE sentence that summarises what they talk about or say to each other.
Do not use quotation marks. Do not use names in the sentence. Just the summary."#,
            a_name        = a.identity.name,
            a_personality = a.identity.personality,
            b_name        = b.identity.name,
            b_personality = b.identity.personality,
            a_mem         = a_mem,
            b_mem         = b_mem,
        )
    }

    // -----------------------------------------------------------------------
    // Available actions for prompt
    // -----------------------------------------------------------------------

    fn available_actions(&self, idx: usize, _is_night: bool) -> Vec<String> {
        let loc   = &self.agents[idx].location;
        let mut v = Vec::new();

        if self.location_allows(loc, "eat")      { v.push("eat".to_string()); }
        if self.location_allows(loc, "cook")     { v.push("cook".to_string()); }
        if *loc == self.agents[idx].home         { v.push("sleep".to_string()); }
        v.push("rest".to_string());
        if self.location_allows(loc, "forage")   { v.push("forage".to_string()); }
        if self.location_allows(loc, "fish")     { v.push("fish".to_string()); }
        if self.location_allows(loc, "exercise") { v.push("exercise".to_string()); }
        if self.location_allows(loc, "bathe")    { v.push("bathe".to_string()); }
        if self.location_allows(loc, "explore")  { v.push("explore".to_string()); }
        if self.location_allows(loc, "play")     { v.push("play".to_string()); }

        // Chat: only if someone else is here and not busy
        let partners: Vec<String> = self.agents.iter()
            .filter(|a| a.id != idx && a.location == *loc && !a.is_busy())
            .map(|a| format!("chat with {}", a.name()))
            .collect();
        v.extend(partners);

        // Move: list adjacent locations
        if let Some(location) = self.locations.get(&self.location_key(loc)) {
            for adj in &location.adjacent {
                v.push(format!("move to {}", self.location_name(adj)));
            }
        }

        v.push("cast_intent (speak a desire)".to_string());
        v
    }

    // -----------------------------------------------------------------------
    // Narrative generation
    // -----------------------------------------------------------------------

    fn narrative_for(&self, res: &Resolution, idx: usize) -> String {
        let name = self.agents[idx].name();
        match res.tier {
            OutcomeTier::CriticalFail => match &res.action {
                Action::Cook    => format!("{} burns everything badly. Still, something edible emerges.", name),
                Action::Forage  => format!("{} gets thoroughly lost but stumbles on a few berries.", name),
                Action::Fish    => format!("{} tangles the line and falls in — but emerges with a small fish.", name),
                Action::Exercise => format!("{} overdoes it and pulls a muscle, but feels the burn.", name),
                _               => format!("{} fumbles badly but manages something.", name),
            },
            OutcomeTier::Fail => match &res.action {
                Action::Cook    => format!("{} produces an inedible mess.", name),
                Action::Forage  => format!("{} searches but finds nothing worth eating.", name),
                Action::Fish    => format!("{} watches the fish ignore every cast.", name),
                Action::Exercise => format!("{} struggles through the routine without benefit.", name),
                Action::Explore  => format!("{} wanders in circles.", name),
                _               => format!("{} attempts it but nothing comes of it.", name),
            },
            OutcomeTier::Success => match &res.action {
                Action::Eat     => format!("{} enjoys a satisfying meal.", name),
                Action::Cook    => format!("{} prepares a delicious dish.", name),
                Action::Rest    => format!("{} rests and feels refreshed.", name),
                Action::Forage  => format!("{} finds plenty of edible plants and berries.", name),
                Action::Fish    => format!("{} hauls in a good catch.", name),
                Action::Exercise => format!("{} completes a solid workout.", name),
                Action::Bathe   => format!("{} emerges clean and refreshed.", name),
                Action::Explore  => format!("{} discovers interesting corners of the forest.", name),
                Action::Play    => format!("{} plays and lifts their spirits.", name),
                _               => format!("{} succeeds.", name),
            },
            OutcomeTier::CriticalSuccess => match &res.action {
                Action::Cook    => format!("{} creates an extraordinary meal — the best in memory!", name),
                Action::Forage  => format!("{} finds an abundance of food, more than expected.", name),
                Action::Fish    => format!("{} lands a magnificent fish with perfect form.", name),
                Action::Exercise => format!("{} exceeds their own expectations — a breakthrough!", name),
                Action::Explore  => format!("{} discovers something truly remarkable in the forest.", name),
                _               => format!("{} exceeds all expectations!", name),
            },
        }
    }

    // -----------------------------------------------------------------------
    // Location helpers
    // -----------------------------------------------------------------------

    fn location_key(&self, loc: &LocationId) -> String {
        format!("{:?}", loc)
    }

    pub fn location_name(&self, loc: &LocationId) -> String {
        match loc {
            LocationId::VillageSquare => "Village Square".to_string(),
            LocationId::Tavern        => "Tavern".to_string(),
            LocationId::Forest        => "Forest".to_string(),
            LocationId::River         => "River".to_string(),
            LocationId::Home(n)       => {
                if let Some(agent) = self.agents.get(*n) {
                    format!("{}'s Home", agent.identity.name)
                } else {
                    "Home".to_string()
                }
            }
        }
    }

    fn location_desc(&self, loc: &LocationId) -> &str {
        match loc {
            LocationId::VillageSquare => "The heart of the village. Open sky, worn cobblestones, familiar faces.",
            LocationId::Tavern        => "A warm, low-ceilinged tavern. The smell of ale and woodsmoke.",
            LocationId::Forest        => "Old trees press close. Birdsong and shadow.",
            LocationId::River         => "A clear river murmurs over stones. Willows trail their fingers in the water.",
            LocationId::Home(_)       => "A small, cosy home. Familiar and safe.",
        }
    }

    fn location_allows(&self, loc: &LocationId, action: &str) -> bool {
        let key = self.location_key(loc);
        self.locations.get(&key).map(|l| l.affordances.contains(&action)).unwrap_or(false)
    }

    fn is_adjacent(&self, from: &LocationId, to: &LocationId) -> bool {
        let key = self.location_key(from);
        self.locations
            .get(&key)
            .map(|l| l.adjacent.contains(to))
            .unwrap_or(false)
    }

    pub fn parse_location(&self, name: &str) -> Option<LocationId> {
        match name.to_lowercase().trim() {
            "village square" | "square"       => Some(LocationId::VillageSquare),
            "tavern"                           => Some(LocationId::Tavern),
            "forest"                           => Some(LocationId::Forest),
            "river"                            => Some(LocationId::River),
            "rowan's home" | "home" | "rowan's" => Some(LocationId::Home(0)),
            "elara's home" | "elara's"         => Some(LocationId::Home(1)),
            "thane's home" | "thane's"         => Some(LocationId::Home(2)),
            _                                  => {
                // Try home(n) pattern
                if name.to_lowercase().contains("home") {
                    // Return own home as a fallback — validated later
                    Some(LocationId::Home(0))
                } else {
                    None
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Build the location graph
// ---------------------------------------------------------------------------

fn build_locations() -> HashMap<String, Location> {
    use LocationId::*;
    let mut m = HashMap::new();

    macro_rules! loc {
        ($id:expr, $name:expr, $desc:expr, $adj:expr, $aff:expr) => {
            m.insert(
                format!("{:?}", $id),
                Location {
                    id:          $id,
                    name:        $name,
                    description: $desc,
                    adjacent:    $adj,
                    affordances: $aff,
                },
            )
        };
    }

    loc!(
        VillageSquare,
        "Village Square",
        "The heart of the village.",
        vec![Forest, River, Tavern, Home(0), Home(1), Home(2)],
        vec!["chat", "exercise", "play", "cast_intent"]
    );
    loc!(
        Tavern,
        "Tavern",
        "A warm tavern.",
        vec![VillageSquare],
        vec!["eat", "cook", "chat", "play", "cast_intent"]
    );
    loc!(
        Forest,
        "Forest",
        "The old forest.",
        vec![VillageSquare],
        vec!["forage", "explore", "exercise", "cast_intent"]
    );
    loc!(
        River,
        "River",
        "A clear river.",
        vec![VillageSquare],
        vec!["fish", "bathe", "rest", "cast_intent"]
    );
    loc!(
        Home(0),
        "Rowan's Home",
        "A cosy home.",
        vec![VillageSquare],
        vec!["eat", "cook", "sleep", "rest", "cast_intent"]
    );
    loc!(
        Home(1),
        "Elara's Home",
        "A cosy home.",
        vec![VillageSquare],
        vec!["eat", "cook", "sleep", "rest", "cast_intent"]
    );
    loc!(
        Home(2),
        "Thane's Home",
        "A cosy home.",
        vec![VillageSquare],
        vec!["eat", "cook", "sleep", "rest", "cast_intent"]
    );

    m
}
