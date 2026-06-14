use std::os::raw::{c_char, c_int};

#[repr(C)]
pub struct LiteRtLmEngine { _unused: [u8; 0] }
#[repr(C)]
pub struct LiteRtLmConversation { _unused: [u8; 0] }
#[repr(C)]
pub struct LiteRtLmConversationOptionalArgs { _unused: [u8; 0] }
#[repr(C)]
pub struct LiteRtLmJsonResponse { _unused: [u8; 0] }
#[repr(C)]
pub struct LiteRtLmEngineSettings { _unused: [u8; 0] }
#[repr(C)]
pub struct LiteRtLmSessionConfig { _unused: [u8; 0] }
#[repr(C)]
pub struct LiteRtLmConversationConfig { _unused: [u8; 0] }

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiteRtLmSamplerType {
    Unspecified = 0,
    TopK = 1,
    TopP = 2,
    Greedy = 3,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct LiteRtLmSamplerParams {
    pub sampler_type: LiteRtLmSamplerType,
    pub top_k: i32,
    pub top_p: f32,
    pub temperature: f32,
    pub seed: i32,
}

unsafe extern "C" {
    pub fn litert_lm_set_min_log_level(level: c_int);

    pub fn litert_lm_engine_settings_create(
        model_path: *const c_char,
        backend_str: *const c_char,
        vision_backend_str: *const c_char,
        audio_backend_str: *const c_char,
    ) -> *mut LiteRtLmEngineSettings;

    pub fn litert_lm_engine_settings_delete(settings: *mut LiteRtLmEngineSettings);

    pub fn litert_lm_engine_settings_set_max_num_tokens(
        settings: *mut LiteRtLmEngineSettings,
        max_num_tokens: c_int,
    );

    pub fn litert_lm_engine_create(
        settings: *const LiteRtLmEngineSettings,
    ) -> *mut LiteRtLmEngine;

    pub fn litert_lm_engine_delete(engine: *mut LiteRtLmEngine);

    pub fn litert_lm_session_config_create() -> *mut LiteRtLmSessionConfig;

    pub fn litert_lm_session_config_delete(config: *mut LiteRtLmSessionConfig);

    pub fn litert_lm_session_config_set_sampler_params(
        config: *mut LiteRtLmSessionConfig,
        sampler_params: *const LiteRtLmSamplerParams,
    );

    pub fn litert_lm_conversation_config_create() -> *mut LiteRtLmConversationConfig;

    pub fn litert_lm_conversation_config_delete(config: *mut LiteRtLmConversationConfig);

    pub fn litert_lm_conversation_config_set_session_config(
        config: *mut LiteRtLmConversationConfig,
        session_config: *const LiteRtLmSessionConfig,
    );

    pub fn litert_lm_conversation_config_set_system_message(
        config: *mut LiteRtLmConversationConfig,
        system_message_json: *const c_char,
    );

    pub fn litert_lm_conversation_create(
        engine: *mut LiteRtLmEngine,
        config: *const LiteRtLmConversationConfig,
    ) -> *mut LiteRtLmConversation;

    pub fn litert_lm_conversation_delete(conversation: *mut LiteRtLmConversation);

    pub fn litert_lm_conversation_send_message(
        conversation: *mut LiteRtLmConversation,
        message_json: *const c_char,
        extra_context: *const c_char,
        optional_args: *const LiteRtLmConversationOptionalArgs,
    ) -> *mut LiteRtLmJsonResponse;

    pub fn litert_lm_conversation_optional_args_create() -> *mut LiteRtLmConversationOptionalArgs;

    pub fn litert_lm_conversation_optional_args_delete(
        optional_args: *mut LiteRtLmConversationOptionalArgs,
    );

    pub fn litert_lm_json_response_delete(response: *mut LiteRtLmJsonResponse);

    pub fn litert_lm_json_response_get_string(
        response: *const LiteRtLmJsonResponse,
    ) -> *const c_char;
}
