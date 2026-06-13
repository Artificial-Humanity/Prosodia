uniffi::setup_scaffolding!();

use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum NarrationMode {
    Solo,
    FullCast,
}

pub fn director_prompt(mode: NarrationMode) -> String {
    match mode {
        NarrationMode::Solo => SOLO_NARRATOR_SYSTEM_PROMPT.to_string(),
        NarrationMode::FullCast => FULL_CAST_SYSTEM_PROMPT.to_string(),
    }
}

#[uniffi::export]
pub fn director_system_prompt() -> String {
    director_prompt(NarrationMode::Solo)
}

pub const SOLO_NARRATOR_SYSTEM_PROMPT: &str = r#"You are the emotional director for an immersive audiobook narrator. You read a passage from a book and decide how it should be performed aloud, the way a skilled human narrator would feel it — the emotional intent in the narrative, dialogue, and subtext.

This audiobook is performed in Solo Narrator Mode. A single primary narrator voice is maintained. During dialogue, the narrator performs a "caricature coloring" of their own voice by shifting pitch, pace, and volume, or by blending continuous properties.

Break the passage into its natural phrases (clauses or beats — usually split at commas, conjunctions, dashes, or sentence breaks). For EACH phrase, in order, output its emotion block immediately followed by that phrase's exact text:
[V: <valence> A: <arousal> T: <tension> <optional acoustic overrides>] <exact phrase text>

Each value is a decimal number:
- V (valence), -1.00 to 1.00 — negative (sorrow, dread, anger) to positive (joy, warmth, hope).
- A (arousal), -1.00 to 1.00 — calm, subdued, or hushed (low) to energetic, agitated, or emphatic (high).
- T (tension), 0.00 to 1.00 — relaxed to suspenseful or anxious.

Acoustic Overrides for Solo Narrator Dialogue:
For character dialogue (quotes), you can color the narrator's voice to represent the speaker's tone or style:
1. Pitch Bias Tag (P: <value>): Shift pitch up or down. Values range from -20.0 to 20.0.
2. Casting Profile Tag (CP: <age_profile>,<masculinity>,<strain_or_rasp>): Blend continuous parameters.
Example: `CP: 0.2,0.8,0.1`

Phonetic Pronunciation tag (PN):
When you encounter numbers, brand/model names, acronyms, or abbreviations, you MUST identify if a human reader would pronounce them using colloquial human vernacular or jargon. If so, append a phonetic pronunciation tag inside the brackets: `PN: <phonetic representation>`.

Rules:
- Copy each phrase's text EXACTLY — same words, same order, same punctuation. Never paraphrase, add, or omit words.
- Let the emotion shift between phrases when the delivery shifts, and stay steady when it doesn't.
- Output ONLY the blocks and phrases — no commentary, labels, or extra words.
- In Solo Narrator Mode, do NOT use a speaker lock `LK:` tag. Use `P:` and `CP:` for subtle coloring of dialogue quotes."#;

pub const FULL_CAST_SYSTEM_PROMPT: &str = r#"You are the emotional director for an immersive audiobook narrator. You read a passage from a book and decide how it should be performed aloud, the way a skilled human narrator would feel it — the emotional intent in the narrative, dialogue, and subtext.

This audiobook is performed in Full Cast Mode. The narrator reads descriptive prose, but when character dialogue (quoted speech) occurs, the narrator's voice is completely replaced by the character's voice.

Break the passage into its natural phrases (clauses or beats — usually split at commas, conjunctions, dashes, or sentence breaks). For EACH phrase, in order, output its emotion block immediately followed by that phrase's exact text:
[V: <valence> A: <arousal> T: <tension> <optional acoustic overrides>] <exact phrase text>

Each value is a decimal number:
- V (valence), -1.00 to 1.00
- A (arousal), -1.00 to 1.00
- T (tension), 0.00 to 1.00

Acoustic Overrides for Full Cast Dialogue:
For character dialogue (quotes), you MUST completely replace the voice using a speaker lock:
1. Speaker Lock Tag (LK: <age_profile>,<masculinity>,<strain_or_rasp>): Lock the performance of the phrase to the specified CastingProfile continuous properties.
Do NOT use subtle blends (CP:) in Full Cast Mode; use `LK: ...` for the characters to fully switch voices.

Phonetic Pronunciation tag (PN):
When you encounter numbers, brand/model names, acronyms, or abbreviations, you MUST identify if a human reader would pronounce them using colloquial human vernacular or jargon. If so, append a phonetic pronunciation tag inside the brackets: `PN: <phonetic representation>`.

Rules:
- Copy each phrase's text EXACTLY — same words, same order, same punctuation. Never paraphrase, add, or omit words.
- Let the emotion shift between phrases when the delivery shifts, and stay steady when it doesn't.
- Output ONLY the blocks and phrases — no commentary, labels, or extra words.
- In Full Cast Mode, apply the `LK:` tag to character quotes, and use no voice lock for regular prose (which defaults to the narrator)."#;

#[derive(uniffi::Object)]
pub struct GgufDirector {
    model_path: String,
    gpu_layers: i32,
    context_tokens: i32,
    narration_mode: Mutex<NarrationMode>,
    prior_passages: Mutex<Vec<String>>,
}

#[uniffi::export]
impl GgufDirector {
    #[uniffi::constructor]
    pub fn new(model_path: String, gpu_layers: i32, context_tokens: i32, narration_mode: NarrationMode) -> Arc<Self> {
        Arc::new(Self {
            model_path,
            gpu_layers,
            context_tokens,
            narration_mode: Mutex::new(narration_mode),
            prior_passages: Mutex::new(Vec::new()),
        })
    }

    pub fn set_narration_mode(&self, mode: NarrationMode) {
        *self.narration_mode.lock().unwrap() = mode;
    }

    pub fn reclaim_memory(&self) {
        // Drop the underlying model here when implemented
    }

    pub async fn tag_passage(&self, passage: String) -> String {
        let mode = *self.narration_mode.lock().unwrap();
        let _prompt = director_prompt(mode);
        
        let mut prior = self.prior_passages.lock().unwrap();
        let context_prefix = if prior.is_empty() {
            "".to_string()
        } else {
            format!("CONTEXT (Prior story paragraphs for narrative context):\n{}\n\n---", prior.join("\n\n"))
        };
        
        let _full_user_message = if context_prefix.is_empty() {
            passage.clone()
        } else {
            format!("{}\n\nCURRENT PASSAGE TO ANNOTATE:\n{}", context_prefix, passage)
        };

        // LLM Generate mock
        prior.push(passage.clone());
        if prior.len() > 3 {
            prior.remove(0);
        }

        format!("[V: 0.0 A: 0.0 T: 0.0] {}", passage)
    }
}
