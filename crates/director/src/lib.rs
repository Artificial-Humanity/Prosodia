uniffi::setup_scaffolding!();

mod ffi;

use std::sync::{Arc, Mutex};
use std::ffi::{CStr, CString};

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

struct EngineWrapper {
    raw: *mut ffi::LiteRtLmEngine,
}

unsafe impl Send for EngineWrapper {}
unsafe impl Sync for EngineWrapper {}

impl Drop for EngineWrapper {
    fn drop(&mut self) {
        unsafe {
            ffi::litert_lm_engine_delete(self.raw);
        }
    }
}

struct ConversationWrapper {
    raw: *mut ffi::LiteRtLmConversation,
}

unsafe impl Send for ConversationWrapper {}
unsafe impl Sync for ConversationWrapper {}

impl Drop for ConversationWrapper {
    fn drop(&mut self) {
        unsafe {
            ffi::litert_lm_conversation_delete(self.raw);
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
struct MessageContent {
    #[serde(rename = "type")]
    content_type: String,
    text: String,
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
struct Message {
    role: String,
    content: Vec<MessageContent>,
}

#[derive(serde::Deserialize, Debug)]
struct ResponseMessage {
    content: Option<Vec<MessageContent>>,
}

/// The on-device director, driven by Gemma 4 via the LiteRT-LM runtime.
///
/// This is the only supported director backend: a Gemma 4 instruct model
/// (`.litertlm`) executed through LiteRT-LM.
#[derive(uniffi::Object)]
pub struct GemmaDirector {
    model_path: String,
    context_tokens: i32,
    narration_mode: Mutex<NarrationMode>,
    prior_passages: Mutex<Vec<String>>,
    engine: Mutex<Option<Arc<EngineWrapper>>>,
}

#[uniffi::export]
impl GemmaDirector {
    #[uniffi::constructor]
    pub fn new(model_path: String, context_tokens: i32, narration_mode: NarrationMode) -> Arc<Self> {
        Arc::new(Self {
            model_path,
            context_tokens,
            narration_mode: Mutex::new(narration_mode),
            prior_passages: Mutex::new(Vec::new()),
            engine: Mutex::new(None),
        })
    }

    pub fn set_narration_mode(&self, mode: NarrationMode) {
        *self.narration_mode.lock().unwrap() = mode;
    }

    pub fn reclaim_memory(&self) {
        *self.engine.lock().unwrap() = None;
        self.prior_passages.lock().unwrap().clear();
    }

    pub fn tag_passage(&self, passage: String) -> String {
        let mode = *self.narration_mode.lock().unwrap();
        let system_prompt = director_prompt(mode);
        
        let full_user_message = {
            let prior = self.prior_passages.lock().unwrap();
            let context_prefix = if prior.is_empty() {
                "".to_string()
            } else {
                format!("CONTEXT (Prior story paragraphs for narrative context):\n{}\n\n---", prior.join("\n\n"))
            };
            if context_prefix.is_empty() {
                passage.clone()
            } else {
                format!("{}\n\nCURRENT PASSAGE TO ANNOTATE:\n{}", context_prefix, passage)
            }
        };

        // Try to perform inference using LiteRT-LM
        match self.generate_inference(&system_prompt, &full_user_message) {
            Ok(annotated_payload) => {
                let mut prior = self.prior_passages.lock().unwrap();
                prior.push(passage.clone());
                if prior.len() > 3 {
                    prior.remove(0);
                }
                annotated_payload
            }
            Err(err) => {
                eprintln!("LiteRT-LM inference error: {}. Falling back to neutral payload.", err);
                format!("[V: 0.0 A: 0.0 T: 0.0] {}", passage)
            }
        }
    }
}

impl GemmaDirector {
    fn get_or_init_engine(&self) -> Result<Arc<EngineWrapper>, String> {
        let mut engine_lock = self.engine.lock().unwrap();
        if let Some(ref engine) = *engine_lock {
            return Ok(Arc::clone(engine));
        }

        unsafe {
            ffi::litert_lm_set_min_log_level(3); // Log level warning

            let model_path_c = CString::new(self.model_path.as_str())
                .map_err(|e| format!("Invalid model path: {}", e))?;
            let backend_c = CString::new("gpu")
                .map_err(|e| format!("Invalid backend: {}", e))?;

            let settings = ffi::litert_lm_engine_settings_create(
                model_path_c.as_ptr(),
                backend_c.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
            );

            if settings.is_null() {
                return Err("Failed to create engine settings".to_string());
            }

            if self.context_tokens > 0 {
                ffi::litert_lm_engine_settings_set_max_num_tokens(settings, self.context_tokens);
            }

            let raw_engine = ffi::litert_lm_engine_create(settings);
            ffi::litert_lm_engine_settings_delete(settings);

            if raw_engine.is_null() {
                return Err("Failed to create LiteRT-LM engine".to_string());
            }

            let wrapper = Arc::new(EngineWrapper { raw: raw_engine });
            *engine_lock = Some(Arc::clone(&wrapper));
            Ok(wrapper)
        }
    }

    fn generate_inference(&self, system_prompt: &str, user_message: &str) -> Result<String, String> {
        let engine = self.get_or_init_engine()?;

        unsafe {
            // 1. Create session config and set sampler parameters
            let session_config = ffi::litert_lm_session_config_create();
            if session_config.is_null() {
                return Err("Failed to create session config".to_string());
            }

            let sampler_params = ffi::LiteRtLmSamplerParams {
                sampler_type: ffi::LiteRtLmSamplerType::TopP,
                top_k: 40,
                top_p: 0.95,
                temperature: 0.3,
                seed: 0,
            };
            ffi::litert_lm_session_config_set_sampler_params(session_config, &sampler_params);

            // 2. Create conversation config
            let conv_config = ffi::litert_lm_conversation_config_create();
            if conv_config.is_null() {
                ffi::litert_lm_session_config_delete(session_config);
                return Err("Failed to create conversation config".to_string());
            }

            ffi::litert_lm_conversation_config_set_session_config(conv_config, session_config);

            // Serialize system prompt to JSON array string
            let sys_content = vec![MessageContent {
                content_type: "text".to_string(),
                text: system_prompt.to_string(),
            }];
            let sys_json = serde_json::to_string(&sys_content)
                .map_err(|e| format!("Failed to serialize system message: {}", e))?;
            let sys_json_c = CString::new(sys_json)
                .map_err(|e| format!("Invalid system message C string: {}", e))?;
            
            ffi::litert_lm_conversation_config_set_system_message(conv_config, sys_json_c.as_ptr());

            // 3. Create conversation
            let raw_conv = ffi::litert_lm_conversation_create(engine.raw, conv_config);
            
            // Cleanup configs
            ffi::litert_lm_conversation_config_delete(conv_config);
            ffi::litert_lm_session_config_delete(session_config);

            if raw_conv.is_null() {
                return Err("Failed to create conversation".to_string());
            }
            let conv = ConversationWrapper { raw: raw_conv };

            // 4. Serialize user message
            let msg = Message {
                role: "user".to_string(),
                content: vec![MessageContent {
                    content_type: "text".to_string(),
                    text: user_message.to_string(),
                }],
            };
            let msg_json = serde_json::to_string(&msg)
                .map_err(|e| format!("Failed to serialize user message: {}", e))?;
            let msg_json_c = CString::new(msg_json)
                .map_err(|e| format!("Invalid user message C string: {}", e))?;

            // 5. Send message
            let optional_args = ffi::litert_lm_conversation_optional_args_create();
            let json_resp = ffi::litert_lm_conversation_send_message(
                conv.raw,
                msg_json_c.as_ptr(),
                std::ptr::null(),
                optional_args,
            );

            if !optional_args.is_null() {
                ffi::litert_lm_conversation_optional_args_delete(optional_args);
            }

            if json_resp.is_null() {
                return Err("Failed to get JSON response from model".to_string());
            }

            let resp_chars = ffi::litert_lm_json_response_get_string(json_resp);
            if resp_chars.is_null() {
                ffi::litert_lm_json_response_delete(json_resp);
                return Err("JSON response string was null".to_string());
            }

            let resp_str = CStr::from_ptr(resp_chars)
                .to_string_lossy()
                .into_owned();

            ffi::litert_lm_json_response_delete(json_resp);

            // 6. Deserialize response
            let response_msg: ResponseMessage = serde_json::from_str(&resp_str)
                .map_err(|e| format!("Failed to deserialize response: {}, raw: {}", e, resp_str))?;

            let text = response_msg.content.unwrap_or_default()
                .iter()
                .filter(|c| c.content_type == "text")
                .map(|c| c.text.as_str())
                .collect::<Vec<_>>()
                .join(" ");

            Ok(text)
        }
    }
}
