use std::sync::Arc;
use stage::prosody_payload::ProsodySpan;
use crate::g2p::{ProsodiaG2PProcessor, TokenPhonemes};
use crate::asset_manager::{ModelAssetManager, StyleVector};

#[derive(Clone, Debug, uniffi::Record)]
pub struct PipelineOutput {
    pub phonemes: Vec<TokenPhonemes>,
    pub style: StyleVector,
    pub speed_multiplier: f64,
    pub gain_multiplier: f64,
}

#[derive(uniffi::Object)]
pub struct ProsodiaActorPipeline {
    g2p: Box<dyn ProsodiaG2PProcessor>,
    asset_manager: Box<dyn ModelAssetManager>,
}

#[uniffi::export]
impl ProsodiaActorPipeline {
    #[uniffi::constructor]
    pub fn new(g2p: Box<dyn ProsodiaG2PProcessor>, asset_manager: Box<dyn ModelAssetManager>) -> Arc<Self> {
        Arc::new(Self { g2p, asset_manager })
    }

    pub fn process_span(&self, span: ProsodySpan) -> PipelineOutput {
        let mtokens = self.g2p.process(span.text);
        let mut phonemes = Vec::new();
        for m in mtokens {
            phonemes.push(TokenPhonemes {
                phonemes: m.phonemes,
                whitespace: " ".to_string(),
            });
        }

        let mut speed = 1.0;
        let mut gain = 1.0;
        let mut style = StyleVector { data: vec![0.0; 64], shape: vec![64] };

        if let Some(acoustics) = span.acoustics {
            if let Some(s) = acoustics.speed_multiplier {
                speed = s;
            }
            if let Some(g) = acoustics.gain_multiplier {
                gain = g;
            }
            if let Some(profile) = acoustics.casting_profile {
                style = self.asset_manager.resolve_casting_profile(profile);
            }
        }

        PipelineOutput {
            phonemes,
            style,
            speed_multiplier: speed,
            gain_multiplier: gain,
        }
    }
}
