use std::collections::VecDeque;
use serde::{Deserialize, Serialize};

use crate::config::{Config, NeedsValues};
use crate::soul::SoulSeed;

pub type AgentId = usize;

// ---------------------------------------------------------------------------
// Identity
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentIdentity {
    pub name:             String,
    pub personality:      String,
    pub backstory:        String,
    pub magical_affinity: String,
    pub self_declaration: String,
}

// ---------------------------------------------------------------------------
// Attributes
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attributes {
    pub vigor: u32,
    pub wit:   u32,
    pub grace: u32,
    pub heart: u32,
    pub numen: u32,
}

impl Attributes {
    /// Returns the d20 modifier for a named attribute.
    pub fn modifier(&self, attr: &str) -> i32 {
        let score = match attr {
            "vigor" => self.vigor,
            "wit"   => self.wit,
            "grace" => self.grace,
            "heart" => self.heart,
            "numen" => self.numen,
            _       => 5,
        } as i32;
        score - 5
    }
}

// ---------------------------------------------------------------------------
// Needs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Needs {
    pub hunger:  f32,
    pub energy:  f32,
    pub fun:     f32,
    pub social:  f32,
    pub hygiene: f32,
}

impl Needs {
    pub fn from_initial(v: &NeedsValues) -> Self {
        Needs { hunger: v.hunger, energy: v.energy, fun: v.fun, social: v.social, hygiene: v.hygiene }
    }

    pub fn clamp(&mut self) {
        self.hunger  = self.hunger .clamp(0.0, 100.0);
        self.energy  = self.energy .clamp(0.0, 100.0);
        self.fun     = self.fun    .clamp(0.0, 100.0);
        self.social  = self.social .clamp(0.0, 100.0);
        self.hygiene = self.hygiene.clamp(0.0, 100.0);
    }

    pub fn apply_decay(&mut self, decay: &NeedsValues) {
        self.hunger  -= decay.hunger;
        self.energy  -= decay.energy;
        self.fun     -= decay.fun;
        self.social  -= decay.social;
        self.hygiene -= decay.hygiene;
        self.clamp();
    }

    pub fn apply(&mut self, changes: &NeedChanges) {
        if let Some(v) = changes.hunger  { self.hunger  += v; }
        if let Some(v) = changes.energy  { self.energy  += v; }
        if let Some(v) = changes.fun     { self.fun     += v; }
        if let Some(v) = changes.social  { self.social  += v; }
        if let Some(v) = changes.hygiene { self.hygiene += v; }
        self.clamp();
    }

    /// Sum of d20 penalties from need states, for the given attribute.
    pub fn penalty(&self, config: &Config, attribute: &str) -> i32 {
        let t = &config.needs.thresholds;
        let mut p = 0i32;

        // Hunger penalises all checks
        if self.hunger < t.penalty_severe {
            p -= 4;
        } else if self.hunger < t.penalty_mild {
            p -= 2;
        }

        // Energy penalises physical checks
        let physical = matches!(attribute, "vigor" | "grace");
        if physical {
            if self.energy < t.penalty_severe {
                p -= 4;
            } else if self.energy < t.penalty_mild {
                p -= 2;
            }
        }

        // Fun: -2 all at <10
        if self.fun < t.penalty_severe {
            p -= 2;
        }

        // Social + Hygiene: -2 Heart at <10
        if attribute == "heart" {
            if self.social  < t.penalty_severe { p -= 2; }
            if self.hygiene < t.penalty_severe { p -= 2; }
        }

        p
    }

    pub fn compact(&self) -> String {
        format!(
            "H:{:.0} E:{:.0} F:{:.0} S:{:.0} Y:{:.0}",
            self.hunger, self.energy, self.fun, self.social, self.hygiene
        )
    }
}

// ---------------------------------------------------------------------------
// NeedChanges — delta applied after action resolution
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NeedChanges {
    pub hunger:  Option<f32>,
    pub energy:  Option<f32>,
    pub fun:     Option<f32>,
    pub social:  Option<f32>,
    pub hygiene: Option<f32>,
}

impl NeedChanges {
    pub fn scale(&self, factor: f32) -> Self {
        NeedChanges {
            hunger:  self.hunger .map(|v| v * factor),
            energy:  self.energy .map(|v| v * factor),
            fun:     self.fun    .map(|v| v * factor),
            social:  self.social .map(|v| v * factor),
            hygiene: self.hygiene.map(|v| v * factor),
        }
    }

    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        let fmt = |label: &str, val: f32| {
            if val > 0.0 { format!("{} +{:.0}", label, val) }
            else         { format!("{} {:.0}", label, val) }
        };
        if let Some(v) = self.hunger  { if v != 0.0 { parts.push(fmt("Hunger",  v)); } }
        if let Some(v) = self.energy  { if v != 0.0 { parts.push(fmt("Energy",  v)); } }
        if let Some(v) = self.fun     { if v != 0.0 { parts.push(fmt("Fun",     v)); } }
        if let Some(v) = self.social  { if v != 0.0 { parts.push(fmt("Social",  v)); } }
        if let Some(v) = self.hygiene { if v != 0.0 { parts.push(fmt("Hygiene", v)); } }
        parts.join(", ")
    }
}

// ---------------------------------------------------------------------------
// Agent
// ---------------------------------------------------------------------------

/// LocationId duplicated here to avoid a circular module dep; world.rs re-exports it.
/// The canonical definition lives in world.rs.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LocationId {
    VillageSquare,
    Tavern,
    Forest,
    River,
    Home(usize),
}

impl LocationId {
    pub fn display_name(&self) -> &str {
        match self {
            LocationId::VillageSquare => "Village Square",
            LocationId::Tavern        => "Tavern",
            LocationId::Forest        => "Forest",
            LocationId::River         => "River",
            LocationId::Home(0)       => "Home (Rowan's)",
            LocationId::Home(1)       => "Home (Elara's)",
            LocationId::Home(2)       => "Home (Thane's)",
            LocationId::Home(n)       => {
                // Static strings for arbitrary indices aren't easy; return a fallback.
                let _ = n;
                "Home"
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub id:                AgentId,
    pub identity:          AgentIdentity,
    pub attributes:        Attributes,
    pub needs:             Needs,
    pub location:          LocationId,
    pub home:              LocationId,
    pub memory:            VecDeque<String>,
    pub busy_ticks:        u32,
    /// Energy restored per tick while sleeping (None when not sleeping).
    pub sleep_energy_tick: Option<f32>,
}

impl Agent {
    pub fn from_soul(id: AgentId, soul: &SoulSeed, config: &Config) -> Self {
        let home = LocationId::Home(id);
        Agent {
            id,
            identity: AgentIdentity {
                name:             soul.name.clone(),
                personality:      soul.personality.clone(),
                backstory:        soul.backstory.clone(),
                magical_affinity: soul.magical_affinity.clone(),
                self_declaration: soul.self_declaration.clone(),
            },
            attributes: Attributes {
                vigor: soul.vigor,
                wit:   soul.wit,
                grace: soul.grace,
                heart: soul.heart,
                numen: soul.numen,
            },
            needs:             Needs::from_initial(&config.needs.initial),
            location:          home.clone(),
            home,
            memory:            VecDeque::new(),
            busy_ticks:        0,
            sleep_energy_tick: None,
        }
    }

    pub fn name(&self) -> &str { &self.identity.name }
    pub fn is_busy(&self) -> bool { self.busy_ticks > 0 }

    pub fn push_memory(&mut self, entry: String, max_size: usize) {
        self.memory.push_front(entry);
        while self.memory.len() > max_size {
            self.memory.pop_back();
        }
    }

    /// Formatted need warnings for the perception prompt.
    pub fn need_warnings(&self, config: &Config) -> Vec<String> {
        let t  = &config.needs.thresholds;
        let mut w = Vec::new();

        if self.needs.hunger < t.forced_action {
            w.push("You are STARVING. You need food immediately.".into());
        } else if self.needs.hunger < t.penalty_severe {
            w.push("You are very hungry. Your body aches for food.".into());
        } else if self.needs.hunger < t.penalty_mild {
            w.push("You are hungry.".into());
        }

        if self.needs.energy < t.forced_action {
            w.push("You are utterly exhausted. You cannot stay awake.".into());
        } else if self.needs.energy < t.penalty_severe {
            w.push("You are exhausted. You can barely keep your eyes open.".into());
        } else if self.needs.energy < t.penalty_mild {
            w.push("You feel tired.".into());
        }

        if self.needs.fun < t.forced_action {
            w.push("A deep, grey boredom has settled over you.".into());
        } else if self.needs.fun < t.penalty_severe {
            w.push("Life feels dull and joyless.".into());
        }

        if self.needs.social < t.forced_action {
            w.push("You feel achingly lonely, desperate for connection.".into());
        } else if self.needs.social < t.penalty_severe {
            w.push("You crave the company of others.".into());
        }

        if self.needs.hygiene < t.penalty_mild {
            w.push("You are becoming quite grimy.".into());
        }

        w
    }
}
