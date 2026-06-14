uniffi::setup_scaffolding!();

pub mod prosody;
pub mod acoustic_matrix;
pub mod audio_shaping;
pub mod prosody_payload;
pub mod director_annotation;
pub mod phrasing;
pub mod coordinator;

pub use prosody::*;
pub use acoustic_matrix::*;
pub use audio_shaping::*;
pub use prosody_payload::*;
pub use director_annotation::*;
pub use phrasing::*;
pub use coordinator::*;
