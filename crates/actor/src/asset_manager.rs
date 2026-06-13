use std::sync::Arc;
use stage::prosody::CastingProfile;

#[derive(Clone, Debug, uniffi::Record)]
pub struct StyleVector {
    pub data: Vec<f32>,
    pub shape: Vec<u32>,
}

#[uniffi::export(callback_interface)]
pub trait ModelAssetManager: Send + Sync {
    fn load_style_anchor(&self, voice_id: String) -> Option<StyleVector>;
    fn resolve_casting_profile(&self, profile: CastingProfile) -> StyleVector;
}

#[derive(uniffi::Object)]
pub struct DefaultModelAssetManager {}

#[uniffi::export]
impl DefaultModelAssetManager {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Self {})
    }
}

#[uniffi::export]
impl ModelAssetManager for DefaultModelAssetManager {
    fn load_style_anchor(&self, _voice_id: String) -> Option<StyleVector> {
        Some(StyleVector { data: vec![0.0; 64], shape: vec![64] })
    }

    fn resolve_casting_profile(&self, profile: CastingProfile) -> StyleVector {
        StyleVector { data: vec![profile.age_profile as f32; 64], shape: vec![64] }
    }
}
