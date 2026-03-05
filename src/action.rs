use rand::rngs::StdRng;
use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::agent::{AgentId, Attributes, NeedChanges, Needs};
use crate::config::{ActionConfig, Config, ResolutionConfig};

// ---------------------------------------------------------------------------
// Action enum
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Action {
    Eat,
    Cook,
    Sleep,
    Rest,
    Forage,
    Fish,
    Exercise,
    Chat { target_name: String },
    Bathe,
    Explore,
    Play,
    Move { destination: String },
    CastIntent { intent: String },
    /// Fallback when requested action fails validation.
    Wander,
}

impl Action {
    pub fn name(&self) -> &'static str {
        match self {
            Action::Eat         => "Eat",
            Action::Cook        => "Cook",
            Action::Sleep       => "Sleep",
            Action::Rest        => "Rest",
            Action::Forage      => "Forage",
            Action::Fish        => "Fish",
            Action::Exercise    => "Exercise",
            Action::Chat { .. } => "Chat",
            Action::Bathe       => "Bathe",
            Action::Explore     => "Explore",
            Action::Play        => "Play",
            Action::Move { .. } => "Move",
            Action::CastIntent { .. } => "Cast Intent",
            Action::Wander      => "Wander",
        }
    }

    pub fn display(&self) -> String {
        match self {
            Action::Chat { target_name }       => format!("Chat with {}", target_name),
            Action::Move { destination }       => format!("Move > {}", destination),
            Action::CastIntent { intent }      => format!("Cast Intent: \"{}\"", intent),
            other => other.name().to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// Outcome tier
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum OutcomeTier {
    CriticalFail,
    Fail,
    Success,
    CriticalSuccess,
}

impl OutcomeTier {
    pub fn label(&self) -> &'static str {
        match self {
            OutcomeTier::CriticalFail    => "Critical Fail",
            OutcomeTier::Fail            => "Fail",
            OutcomeTier::Success         => "Success",
            OutcomeTier::CriticalSuccess => "Critical Success",
        }
    }

    /// Multiplier applied to need changes.
    pub fn multiplier(&self) -> f32 {
        match self {
            OutcomeTier::CriticalFail    => 0.5,
            OutcomeTier::Fail            => 0.0,
            OutcomeTier::Success         => 1.0,
            OutcomeTier::CriticalSuccess => 1.5,
        }
    }
}

// ---------------------------------------------------------------------------
// Action resolution result
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Resolution {
    pub action:       Action,
    pub tier:         OutcomeTier,
    pub roll:         u32,
    pub modifier:     i32,
    pub penalty:      i32,
    pub total:        i32,
    pub dc:           u32,
    pub need_changes: NeedChanges,
    pub duration:     u32,
    pub narrative:    String,
}

impl Resolution {
    pub fn is_auto_success(cfg: &ActionConfig) -> bool {
        cfg.dc == 0
    }

    pub fn check_line(&self) -> String {
        if self.dc == 0 { return String::new(); }
        let attr = self.attribute_label();
        if attr.is_empty() {
            format!("{} vs DC {} | {}", self.total, self.dc, self.tier.label())
        } else {
            format!("{} {} vs DC {} | {}", attr, self.total, self.dc, self.tier.label())
        }
    }

    fn attribute_label(&self) -> String {
        match &self.action {
            Action::Cook     => "Wit".into(),
            Action::Forage   => "Grace".into(),
            Action::Fish     => "Grace".into(),
            Action::Exercise => "Vigor".into(),
            Action::Chat { .. } => "Heart".into(),
            Action::Explore  => "Vigor".into(),
            _ => String::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Resolution logic
// ---------------------------------------------------------------------------

/// Build base NeedChanges from an ActionConfig.
fn base_changes(cfg: &ActionConfig) -> NeedChanges {
    NeedChanges {
        hunger:  cfg.hunger_restore,
        energy:  cfg.energy_restore.map(|v| v).or_else(|| cfg.energy_drain.map(|d| -d)),
        fun:     cfg.fun_restore,
        social:  cfg.social_restore,
        hygiene: cfg.hygiene_restore,
    }
}

/// Resolve a non-magic action. Returns a Resolution.
pub fn resolve(
    action:     &Action,
    attributes: &Attributes,
    needs:      &Needs,
    config:     &Config,
    is_night:   bool,
    rng:        &mut StdRng,
) -> Resolution {
    let (cfg, attr_name) = action_cfg_and_attr(action, config);
    let dc               = effective_dc(action, cfg, is_night, config);
    let base             = base_changes(cfg);

    // Auto-success actions (dc = 0)
    if dc == 0 {
        let duration = cfg.duration_ticks.unwrap_or(1);
        return Resolution {
            action: action.clone(),
            tier: OutcomeTier::Success,
            roll: 0, modifier: 0, penalty: 0, total: 0, dc: 0,
            need_changes: base,
            duration,
            narrative: auto_narrative(action),
        };
    }

    let roll     = rng.gen_range(1u32..=20);
    let modifier = attributes.modifier(attr_name);
    let penalty  = needs.penalty(config, attr_name);
    let total    = roll as i32 + modifier + penalty;

    let tier = if roll == config.resolution.crit_fail {
        OutcomeTier::CriticalFail
    } else if roll == config.resolution.crit_success {
        OutcomeTier::CriticalSuccess
    } else if total >= dc as i32 {
        OutcomeTier::Success
    } else {
        OutcomeTier::Fail
    };

    let need_changes = base.scale(tier.multiplier());

    Resolution {
        action: action.clone(),
        tier,
        roll, modifier, penalty, total, dc,
        need_changes,
        duration: 1,
        narrative: String::new(), // filled by world/log layer
    }
}

fn effective_dc(action: &Action, cfg: &ActionConfig, is_night: bool, config: &Config) -> u32 {
    let base = cfg.dc;
    if base == 0 { return 0; }
    let night_bonus = match action {
        Action::Forage | Action::Explore if is_night => config.resolution.night_dc_bonus as u32,
        _ => 0,
    };
    base + night_bonus
}

fn auto_narrative(action: &Action) -> String {
    match action {
        Action::Eat   => "Takes a simple meal.".into(),
        Action::Rest  => "Rests for a while.".into(),
        Action::Sleep => "Falls into a deep sleep.".into(),
        Action::Bathe => "Washes away the day's grime.".into(),
        Action::Play  => "Plays lightheartedly.".into(),
        Action::Move { destination } => format!("Moves to {}.", destination),
        Action::Wander => "Wanders to an adjacent location.".into(),
        _ => "Acts.".into(),
    }
}

/// Returns (ActionConfig, attribute_name) for the given action.
pub fn action_cfg_and_attr<'a>(action: &Action, config: &'a Config) -> (&'a ActionConfig, &'static str) {
    match action {
        Action::Eat         => (&config.actions.eat,         ""),
        Action::Cook        => (&config.actions.cook,        "wit"),
        Action::Sleep       => (&config.actions.sleep,       ""),
        Action::Rest        => (&config.actions.rest,        ""),
        Action::Forage      => (&config.actions.forage,      "grace"),
        Action::Fish        => (&config.actions.fish,        "grace"),
        Action::Exercise    => (&config.actions.exercise,    "vigor"),
        Action::Chat { .. } => (&config.actions.chat,        "heart"),
        Action::Bathe       => (&config.actions.bathe,       ""),
        Action::Explore     => (&config.actions.explore,     "vigor"),
        Action::Play        => (&config.actions.play,        ""),
        Action::Move { .. } => (&config.actions.rest,        ""), // placeholder; move has no needs
        Action::CastIntent{ .. } => (&config.actions.cast_intent, "numen"),
        Action::Wander      => (&config.actions.rest,        ""),
    }
}

// ---------------------------------------------------------------------------
// Parse LLM response JSON into an Action
// ---------------------------------------------------------------------------

#[derive(Deserialize, Default)]
struct ActionResponse {
    action: Option<String>,
    target: Option<String>,
    intent: Option<String>,
    #[allow(dead_code)]
    reason: Option<String>,
}

/// Cascading parser: JSON → code-fence extraction → regex → Wander default.
pub fn parse_response(raw: &str) -> Action {
    // 1. Try direct JSON parse
    if let Some(a) = try_parse_json(raw) {
        return a;
    }
    // 2. Extract from ```json ... ``` code fence
    if let Some(json) = extract_code_fence(raw) {
        if let Some(a) = try_parse_json(&json) {
            return a;
        }
    }
    // 3. Extract action name with regex-like scan
    if let Some(action_name) = extract_action_field(raw) {
        return action_from_name(&action_name, None, None);
    }
    // 4. Default
    tracing::warn!("Could not parse LLM response, defaulting to Wander. Raw: {}", &raw[..raw.len().min(200)]);
    Action::Wander
}

fn try_parse_json(s: &str) -> Option<Action> {
    let s = s.trim();
    let parsed: ActionResponse = serde_json::from_str(s).ok()?;
    let name = parsed.action?;
    Some(action_from_name(&name, parsed.target.as_deref(), parsed.intent.as_deref()))
}

fn extract_code_fence(s: &str) -> Option<String> {
    let start = s.find("```")?;
    let rest  = &s[start + 3..];
    // skip optional language tag
    let rest  = rest.trim_start_matches(|c: char| c.is_alphabetic());
    let end   = rest.find("```")?;
    Some(rest[..end].trim().to_string())
}

fn extract_action_field(s: &str) -> Option<String> {
    // Look for "action": "something"
    let key = "\"action\"";
    let pos  = s.find(key)?;
    let rest = &s[pos + key.len()..];
    let colon = rest.find(':')? + 1;
    let rest  = rest[colon..].trim();
    if rest.starts_with('"') {
        let inner = &rest[1..];
        let end   = inner.find('"')?;
        return Some(inner[..end].to_string());
    }
    None
}

pub fn action_from_name(name: &str, target: Option<&str>, intent: Option<&str>) -> Action {
    match name.to_lowercase().replace('_', " ").trim() {
        "eat"                         => Action::Eat,
        "cook"                        => Action::Cook,
        "sleep"                       => Action::Sleep,
        "rest"                        => Action::Rest,
        "forage"                      => Action::Forage,
        "fish"                        => Action::Fish,
        "exercise"                    => Action::Exercise,
        "bathe"                       => Action::Bathe,
        "explore"                     => Action::Explore,
        "play"                        => Action::Play,
        "wander"                      => Action::Wander,
        "chat" => {
            let t = target.unwrap_or("").to_string();
            Action::Chat { target_name: t }
        }
        "move" => {
            let d = target.unwrap_or("Village Square").to_string();
            Action::Move { destination: d }
        }
        "cast intent" | "cast_intent" => {
            let i = intent.unwrap_or("").to_string();
            if i.is_empty() {
                tracing::warn!("cast_intent action with no intent string — treating as Wander");
                Action::Wander
            } else {
                Action::CastIntent { intent: i }
            }
        }
        other => {
            tracing::warn!("Unknown action '{}', defaulting to Wander", other);
            Action::Wander
        }
    }
}
