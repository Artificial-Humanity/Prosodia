//! Pull-based stage coordination.
//!
//! The coordinator unifies the narration lifecycle —
//! `narration source → director → actor → audio` — into a single Rust object that
//! the platform layer drives by repeatedly calling [`StageCoordinator::next_chunk`].
//!
//! It is deliberately **pull-based and runtime-free**: there is no async runtime,
//! no internal threads, and no lookahead buffer inside Rust. The platform owns the
//! loop and the audio device, so backpressure falls out naturally — Swift/Android
//! call `next_chunk()` only when they are ready for more audio, which paces the
//! Director and Actor without any explicit channel.
//!
//! Every moving part the coordinator touches (pulling the next passage, running the
//! on-device Director LLM, running the Actor synthesizer) is a Swift/Android
//! responsibility expressed as a synchronous callback. The coordinator itself is
//! pure glue, so it depends on no other crate — just the three traits below.

use std::sync::{Arc, Mutex};

/// Supplies the next passage of text to narrate, or `None` when the book is done.
///
/// Implemented by the platform over a parsed `BookDocument` (sentence segmentation
/// and paragraph grouping live on that side for now).
#[uniffi::export(callback_interface)]
pub trait NarrationSource: Send + Sync {
    fn next_passage(&self) -> Option<String>;
}

/// Annotates a passage with emotion/prosody, returning the encoded prosody payload.
///
/// Implemented by the platform around the on-device Gemma director (LiteRT-LM).
#[uniffi::export(callback_interface)]
pub trait DirectorInference: Send + Sync {
    fn annotate(&self, passage: String) -> String;
}

/// Synthesizes an annotated prosody payload into PCM audio samples.
///
/// Implemented by the platform around the StyleTTS2 actor (LiteRT).
#[uniffi::export(callback_interface)]
pub trait VocalActor: Send + Sync {
    fn render(&self, payload: String) -> Vec<f32>;
}

/// One unit of narration: a passage, its annotated prosody payload, and the rendered
/// PCM audio, tagged with its zero-based emission index.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AudioChunk {
    pub passage: String,
    pub payload: String,
    pub audio: Vec<f32>,
    pub sample_rate: u32,
    pub index: u32,
}

#[derive(Default)]
struct CoordinatorState {
    emitted: u32,
    stopped: bool,
}

/// Drives the narration pipeline one chunk at a time.
#[derive(uniffi::Object)]
pub struct StageCoordinator {
    source: Box<dyn NarrationSource>,
    director: Box<dyn DirectorInference>,
    actor: Box<dyn VocalActor>,
    sample_rate: u32,
    state: Mutex<CoordinatorState>,
}

#[uniffi::export]
impl StageCoordinator {
    #[uniffi::constructor]
    pub fn new(
        source: Box<dyn NarrationSource>,
        director: Box<dyn DirectorInference>,
        actor: Box<dyn VocalActor>,
        sample_rate: u32,
    ) -> Arc<Self> {
        Arc::new(Self {
            source,
            director,
            actor,
            sample_rate,
            state: Mutex::new(CoordinatorState::default()),
        })
    }

    /// Pull the next passage, annotate it, synthesize it, and return the result.
    ///
    /// Returns `None` once the narration source is exhausted or [`stop`] has been
    /// called. Blocks for as long as the director and actor callbacks take, so the
    /// platform should call this off the main thread.
    ///
    /// [`stop`]: StageCoordinator::stop
    pub fn next_chunk(&self) -> Option<AudioChunk> {
        if self.state.lock().unwrap().stopped {
            return None;
        }

        let passage = self.source.next_passage()?;
        let payload = self.director.annotate(passage.clone());
        let audio = self.actor.render(payload.clone());

        // Re-check stop: the caller may have stopped us while the (slow) director and
        // actor callbacks were running, in which case this chunk is discarded.
        let mut state = self.state.lock().unwrap();
        if state.stopped {
            return None;
        }
        let index = state.emitted;
        state.emitted += 1;

        Some(AudioChunk {
            passage,
            payload,
            audio,
            sample_rate: self.sample_rate,
            index,
        })
    }

    /// Stop the session. Subsequent calls to [`next_chunk`] return `None`.
    ///
    /// [`next_chunk`]: StageCoordinator::next_chunk
    pub fn stop(&self) {
        self.state.lock().unwrap().stopped = true;
    }

    /// Number of chunks emitted so far.
    pub fn chunks_emitted(&self) -> u32 {
        self.state.lock().unwrap().emitted
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Yields a fixed list of passages once each, in order.
    struct VecSource {
        passages: Mutex<std::collections::VecDeque<String>>,
    }
    impl VecSource {
        fn new(items: &[&str]) -> Self {
            Self {
                passages: Mutex::new(items.iter().map(|s| s.to_string()).collect()),
            }
        }
    }
    impl NarrationSource for VecSource {
        fn next_passage(&self) -> Option<String> {
            self.passages.lock().unwrap().pop_front()
        }
    }

    /// Wraps each passage in a trivial prosody payload.
    struct EchoDirector;
    impl DirectorInference for EchoDirector {
        fn annotate(&self, passage: String) -> String {
            format!("[V: 0.0 A: 0.0 T: 0.0] {passage}")
        }
    }

    /// Emits one sample per character of the payload so tests can assert on length.
    struct LenActor {
        calls: Arc<AtomicUsize>,
    }
    impl VocalActor for LenActor {
        fn render(&self, payload: String) -> Vec<f32> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            vec![1.0; payload.chars().count()]
        }
    }

    fn coordinator(items: &[&str], calls: Arc<AtomicUsize>) -> Arc<StageCoordinator> {
        StageCoordinator::new(
            Box::new(VecSource::new(items)),
            Box::new(EchoDirector),
            Box::new(LenActor { calls }),
            24_000,
        )
    }

    #[test]
    fn emits_chunks_in_order_then_none() {
        let calls = Arc::new(AtomicUsize::new(0));
        let coord = coordinator(&["one", "two"], calls.clone());

        let c0 = coord.next_chunk().unwrap();
        assert_eq!(c0.index, 0);
        assert_eq!(c0.passage, "one");
        assert_eq!(c0.payload, "[V: 0.0 A: 0.0 T: 0.0] one");
        assert_eq!(c0.audio.len(), c0.payload.chars().count());
        assert_eq!(c0.sample_rate, 24_000);

        let c1 = coord.next_chunk().unwrap();
        assert_eq!(c1.index, 1);
        assert_eq!(c1.passage, "two");

        assert!(coord.next_chunk().is_none());
        assert_eq!(coord.chunks_emitted(), 2);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn stop_halts_emission() {
        let calls = Arc::new(AtomicUsize::new(0));
        let coord = coordinator(&["a", "b", "c"], calls.clone());

        assert!(coord.next_chunk().is_some());
        coord.stop();
        assert!(coord.next_chunk().is_none());
        assert_eq!(coord.chunks_emitted(), 1);
        // The director/actor were not invoked again after stop.
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn empty_source_emits_nothing() {
        let calls = Arc::new(AtomicUsize::new(0));
        let coord = coordinator(&[], calls.clone());
        assert!(coord.next_chunk().is_none());
        assert_eq!(coord.chunks_emitted(), 0);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
}
