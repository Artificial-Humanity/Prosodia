use std::sync::Arc;
use crate::pipeline::PipelineOutput;

#[derive(Clone, Debug, uniffi::Record)]
pub struct ActorEngineOutput {
    pub audio: Vec<f32>,
    pub pred_dur: Vec<i32>,
}

#[uniffi::export(callback_interface)]
pub trait ProsodiaSpeechEngine: Send + Sync {
    fn synthesize(&self, input: PipelineOutput) -> ActorEngineOutput;
}

#[uniffi::export(callback_interface)]
pub trait AudioSink: Send + Sync {
    fn schedule_audio(&self, audio: Vec<f32>, sample_rate: u32);
}

#[derive(uniffi::Object)]
pub struct ProsodiaActorEngine {
    pub pipeline: Arc<crate::pipeline::ProsodiaActorPipeline>,
    pub speech_engine: Box<dyn ProsodiaSpeechEngine>,
}

#[uniffi::export]
impl ProsodiaActorEngine {
    #[uniffi::constructor]
    pub fn new(pipeline: Arc<crate::pipeline::ProsodiaActorPipeline>, speech_engine: Box<dyn ProsodiaSpeechEngine>) -> Arc<Self> {
        Arc::new(Self { pipeline, speech_engine })
    }

    pub fn process_and_synthesize(&self, span: stage::prosody_payload::ProsodySpan) -> ActorEngineOutput {
        let pipeline_out = self.pipeline.process_span(span);
        self.speech_engine.synthesize(pipeline_out)
    }
}
