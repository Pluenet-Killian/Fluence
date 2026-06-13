// SPDX-License-Identifier: Apache-2.0

//! Context assembly (SPEC §5.C): the prompt the engine sends, ordered from
//! most stable to most volatile to maximise KV-cache prefix reuse.
//!
//! Five blocks are specified — system+style, injected memory, rolling summary,
//! recent dated turns, draft+instruction. Memory and the rolling summary are
//! the §5.B subsystem (P2) and stay empty in v0, so this assembles blocks 1, 4
//! and 5: the system/style block, the recent turns (relative-dated, truncated
//! to fit the token budget), and the draft with the mode instruction.

use fluence_protocol::api::suggest::{SuggestConstraints, SuggestMode};

use crate::tokens::estimate_tokens;

/// Total context budget in (estimated) tokens (SPEC §5.C: ≤ 2200).
pub const DEFAULT_BUDGET_TOKENS: usize = 2200;

/// Who spoke a context turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Speaker {
    /// The person we accelerate.
    User,
    /// Their interlocutor.
    Partner,
}

impl Speaker {
    /// Display label used in the prompt's turn lines.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::User => "Moi",
            Self::Partner => "Interlocuteur",
        }
    }
}

/// One recent conversation turn, with how long ago it was said.
#[derive(Debug, Clone)]
pub struct ContextTurn {
    /// Who spoke.
    pub speaker: Speaker,
    /// What was said.
    pub text: String,
    /// Seconds elapsed since the turn (for relative dating, §5.C).
    pub seconds_ago: u64,
}

/// The style block (block 1) — how the person speaks (SPEC §5, profil express).
#[derive(Debug, Clone, Default)]
pub struct StyleProfile {
    /// Register hint (`famille`, `formel`…); rendered into the system block.
    pub register: Option<String>,
}

/// Everything needed to assemble a prompt for one suggestion request.
#[derive(Debug, Clone)]
pub struct ContextParts {
    /// Which acceleration function (decides the block-5 instruction).
    pub mode: SuggestMode,
    /// The person's style (block 1).
    pub style: StyleProfile,
    /// Recent turns, oldest first (block 4); the assembler keeps the newest
    /// that fit the budget.
    pub turns: Vec<ContextTurn>,
    /// Current draft (block 5) — tolerant to eye-typing noise.
    pub draft: String,
    /// Optional generation constraints (register hint reinforces the style).
    pub constraints: Option<SuggestConstraints>,
}

/// An assembled prompt plus the budget accounting that produced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssembledPrompt {
    /// The full prompt text.
    pub text: String,
    /// Estimated token count (budget-only — see [`crate::estimate_tokens`]).
    pub estimated_tokens: usize,
    /// How many of the supplied turns survived truncation.
    pub included_turns: usize,
}

/// French relative-time label for a turn said `seconds_ago` ago (SPEC §5.C).
#[must_use]
pub fn relative_label(seconds_ago: u64) -> String {
    const MINUTE: u64 = 60;
    const HOUR: u64 = 60 * MINUTE;
    const DAY: u64 = 24 * HOUR;
    if seconds_ago < MINUTE {
        "à l'instant".to_owned()
    } else if seconds_ago < HOUR {
        format!("il y a {} min", seconds_ago / MINUTE)
    } else if seconds_ago < DAY {
        format!("il y a {} h", seconds_ago / HOUR)
    } else if seconds_ago < 2 * DAY {
        "hier".to_owned()
    } else {
        format!("il y a {} j", seconds_ago / DAY)
    }
}

/// The block-1 system + style instruction for `mode`.
fn system_block(
    mode: SuggestMode,
    style: &StyleProfile,
    constraints: Option<&SuggestConstraints>,
) -> String {
    let mut block = String::from(
        "Tu es l'assistant de communication de la personne. Tu écris en \
         français, dans son style, une phrase claire et naturelle. Tu ne \
         parles pas à sa place : tu proposes, elle décide.",
    );
    match mode {
        SuggestMode::Rephrase => {
            block.push_str(
                " Reformule le brouillon en une seule phrase, en corrigeant les \
                 fautes de frappe. Réponds uniquement par la phrase corrigée, \
                 sans guillemets ni explication.",
            );
        }
        SuggestMode::Continue => block.push_str(" Continue le brouillon dans le même élan."),
        SuggestMode::Replies => block.push_str(" Propose une réponse brève à l'interlocuteur."),
        SuggestMode::Expand => block.push_str(" Développe l'abréviation du brouillon."),
    }
    let register = constraints
        .and_then(|c| c.register.as_deref())
        .or(style.register.as_deref());
    if let Some(register) = register {
        block.push_str(" Registre : ");
        block.push_str(register);
        block.push('.');
    }
    block
}

/// The block-5 instruction that carries the current draft.
fn mode_instruction(mode: SuggestMode, draft: &str) -> String {
    match mode {
        SuggestMode::Rephrase => format!("Brouillon à reformuler : « {draft} »"),
        SuggestMode::Continue => format!("Brouillon à continuer : « {draft} »"),
        SuggestMode::Replies => format!("Dernier message reçu, à qui répondre : « {draft} »"),
        SuggestMode::Expand => format!("Abréviation à développer : « {draft} »"),
    }
}

/// Renders one turn line: `Interlocuteur (il y a 2 min) : …`.
fn render_turn(turn: &ContextTurn) -> String {
    format!(
        "{} ({}) : {}",
        turn.speaker.label(),
        relative_label(turn.seconds_ago),
        turn.text
    )
}

/// Assembles the prompt for `parts`, keeping the newest turns that fit
/// `budget_tokens` (SPEC §5.C). The system block and the draft instruction are
/// always included (even if they alone exceed the budget); only the recent
/// turns are truncated, oldest first.
#[must_use]
pub fn assemble(parts: &ContextParts, budget_tokens: usize) -> AssembledPrompt {
    let system = system_block(parts.mode, &parts.style, parts.constraints.as_ref());
    let instruction = mode_instruction(parts.mode, &parts.draft);

    // Blocks 1 and 5 are mandatory; spend the rest of the budget on turns,
    // newest first, then restore chronological order.
    let mut used = estimate_tokens(&system) + estimate_tokens(&instruction);
    let mut kept: Vec<String> = Vec::new();
    for turn in parts.turns.iter().rev() {
        let line = render_turn(turn);
        let cost = estimate_tokens(&line);
        if used + cost > budget_tokens {
            break;
        }
        used += cost;
        kept.push(line);
    }
    kept.reverse();
    let included_turns = kept.len();

    let mut sections: Vec<String> = vec![system];
    if !kept.is_empty() {
        sections.push(kept.join("\n"));
    }
    sections.push(instruction);
    let text = sections.join("\n\n");

    AssembledPrompt {
        estimated_tokens: estimate_tokens(&text),
        included_turns,
        text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(speaker: Speaker, text: &str, seconds_ago: u64) -> ContextTurn {
        ContextTurn {
            speaker,
            text: text.to_owned(),
            seconds_ago,
        }
    }

    #[test]
    fn relative_labels_follow_spec_5c() {
        assert_eq!(relative_label(0), "à l'instant");
        assert_eq!(relative_label(59), "à l'instant");
        assert_eq!(relative_label(120), "il y a 2 min");
        assert_eq!(relative_label(3 * 3600), "il y a 3 h");
        assert_eq!(relative_label(25 * 3600), "hier");
        assert_eq!(relative_label(3 * 86400), "il y a 3 j");
    }

    #[test]
    fn rephrase_prompt_matches_the_golden() {
        // T1 golden — the exact assembled prompt is human-reviewed (PLAN 4.4).
        let parts = ContextParts {
            mode: SuggestMode::Rephrase,
            style: StyleProfile {
                register: Some("famille".to_owned()),
            },
            turns: vec![turn(Speaker::Partner, "Tu veux quoi pour le dîner ?", 120)],
            draft: "veu eau frache ce soir".to_owned(),
            constraints: None,
        };
        let prompt = assemble(&parts, DEFAULT_BUDGET_TOKENS);
        let expected = "Tu es l'assistant de communication de la personne. Tu écris en \
             français, dans son style, une phrase claire et naturelle. Tu ne \
             parles pas à sa place : tu proposes, elle décide. Reformule le \
             brouillon en une seule phrase, en corrigeant les fautes de frappe. \
             Réponds uniquement par la phrase corrigée, sans guillemets ni \
             explication. Registre : famille.\n\n\
             Interlocuteur (il y a 2 min) : Tu veux quoi pour le dîner ?\n\n\
             Brouillon à reformuler : « veu eau frache ce soir »";
        assert_eq!(prompt.text, expected);
        assert_eq!(prompt.included_turns, 1);
    }

    #[test]
    fn constraints_register_overrides_the_style_register() {
        let parts = ContextParts {
            mode: SuggestMode::Continue,
            style: StyleProfile {
                register: Some("famille".to_owned()),
            },
            turns: vec![],
            draft: "bonjour".to_owned(),
            constraints: Some(SuggestConstraints {
                max_chars: None,
                register: Some("formel".to_owned()),
            }),
        };
        let prompt = assemble(&parts, DEFAULT_BUDGET_TOKENS);
        assert!(prompt.text.contains("Registre : formel."));
        assert!(!prompt.text.contains("famille"));
    }

    #[test]
    fn turns_are_truncated_oldest_first_to_fit_the_budget() {
        // A tiny budget keeps only the newest turn(s); the draft + system stay.
        let turns: Vec<ContextTurn> = (0..50)
            .map(|i| {
                turn(
                    Speaker::Partner,
                    "une phrase de contexte assez longue",
                    60 * (i + 1),
                )
            })
            .collect();
        let parts = ContextParts {
            mode: SuggestMode::Rephrase,
            style: StyleProfile::default(),
            turns,
            draft: "salut".to_owned(),
            constraints: None,
        };
        let prompt = assemble(&parts, 120);
        assert!(prompt.estimated_tokens <= 120);
        assert!(prompt.included_turns < 50, "older turns must be dropped");
        assert!(
            prompt.text.contains("Brouillon à reformuler"),
            "block 5 always kept"
        );
    }

    #[test]
    fn the_newest_turn_survives_truncation() {
        let parts = ContextParts {
            mode: SuggestMode::Rephrase,
            style: StyleProfile::default(),
            turns: vec![
                turn(Speaker::Partner, "vieux message", 3600),
                turn(Speaker::User, "message recent", 30),
            ],
            draft: "ok".to_owned(),
            constraints: None,
        };
        // Budget for system + instruction + exactly one turn line.
        let prompt = assemble(&parts, 120);
        assert!(prompt.text.contains("message recent"), "newest turn kept");
    }
}
