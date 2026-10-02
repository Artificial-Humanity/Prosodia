uniffi::setup_scaffolding!();

pub mod asset_manager;
pub mod chunking;
pub mod device_g2p;
pub mod engine;
pub mod g2p;
pub mod pipeline;
pub mod voice_loader;
pub mod normalization;
pub mod tagger;
pub mod lexicon;
pub mod tflite;
pub mod split_engine;

#[cfg(test)]
mod model_pins;

#[cfg(test)]
mod g2p_parity;

