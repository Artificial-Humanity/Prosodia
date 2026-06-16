//! Pull-based stage coordination.
//!
//! The coordinator unifies the narration lifecycle —
//! `narration source → director → actor → audio` — into a single Rust object that
//! the platform layer drives by repeatedly calling [`StageCoordinator::next_chunk`].
//!
//! By default, it is **pull-based and thread-free**: there is no async runtime,
//! no internal threads, and no lookahead buffer inside Rust. The platform owns the
//! loop and the audio device, so backpressure falls out naturally — Swift/Android
//! call `next_chunk()` only when they are ready for more audio.
//!
//! Optionally, a bounded lookahead limit can be configured via [`StageCoordinator::new_with_lookahead`].
//! In this mode, the coordinator spawns a single background worker thread to eagerly
//! pre-render passages up to the limit, ensuring gap-free playback while avoiding
//! excessive resource consumption.
//!
//! Every moving part the coordinator touches (pulling the next passage, running the
//! on-device Director LLM, running the Actor synthesizer) is a Swift/Android
//! responsibility expressed as a synchronous callback.

use std::sync::{Arc, Mutex, Condvar};

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
    produced: u32,
    stopped: bool,
    source_exhausted: bool,
    chunk_buffer: std::collections::VecDeque<String>,
    last_speaker: Option<String>,
    pre_rendered_queue: std::collections::VecDeque<AudioChunk>,
}

fn run_worker(
    state: Arc<(Mutex<CoordinatorState>, Condvar)>,
    source: Arc<dyn NarrationSource>,
    director: Arc<dyn DirectorInference>,
    actor: Arc<dyn VocalActor>,
    grouping: crate::segmenter::NarrationGrouping,
    sample_rate: u32,
    lookahead_limit: usize,
) {
    let (lock, cvar) = &*state;
    
    let run_loop = || {
        loop {
            let mut state_guard = lock.lock().unwrap();
            
            // Wait if queue is full
            while !state_guard.stopped && state_guard.pre_rendered_queue.len() >= lookahead_limit {
                state_guard = cvar.wait(state_guard).unwrap();
            }
            
            if state_guard.stopped {
                break;
            }
            
            // If we don't have chunks in buffer, and source is not exhausted, fetch the next passage
            if state_guard.chunk_buffer.is_empty() && !state_guard.source_exhausted {
                // Drop lock before calling blocking FFI next_passage
                drop(state_guard);
                
                let maybe_passage = source.next_passage();
                
                state_guard = lock.lock().unwrap();
                if state_guard.stopped {
                    break;
                }
                
                match maybe_passage {
                    Some(raw_passage) => {
                        let sentences = crate::segmenter::split_sentences(&raw_passage);
                        let grouped = grouping.group(&sentences);
                        state_guard.chunk_buffer.extend(grouped);
                    }
                    None => {
                        state_guard.source_exhausted = true;
                        cvar.notify_all();
                    }
                }
            }
            
            // If chunk_buffer is still empty and source is exhausted, we are done producing
            if state_guard.chunk_buffer.is_empty() && state_guard.source_exhausted {
                break;
            }
            
            // Pop the next passage to render
            let passage = match state_guard.chunk_buffer.pop_front() {
                Some(p) => p,
                None => continue,
            };
            
            // Drop lock before calling expensive Director and Actor FFI render functions
            drop(state_guard);
            
            let payload = director.annotate(passage.clone());
            
            // Apply boundary mitigations
            let mut mitigated_payload = payload.clone();
            let mut first_span_leading_pause = 0.0;
            
            if let Some(decoded) = crate::prosody_payload::decode_spans(&payload) {
                let mut spans = decoded.spans;
                spans = crate::segmenter::apply_boundary_mitigations(spans);
                
                // Re-lock briefly to read/write last_speaker
                let mut state_guard = lock.lock().unwrap();
                if state_guard.stopped {
                    break;
                }
                let prev_speaker = state_guard.last_speaker.clone();
                
                if !spans.is_empty() {
                    let first_speaker = spans[0].acoustics.as_ref().and_then(|a| a.speaker_lock.clone());
                    if prev_speaker != first_speaker {
                        if state_guard.produced > 0 {
                            spans[0].leading_pause = spans[0].leading_pause.max(0.25);
                        }
                    }
                    state_guard.last_speaker = spans.last().and_then(|s| s.acoustics.as_ref().and_then(|a| a.speaker_lock.clone()));
                }
                drop(state_guard);
                
                if let Some(first_span) = spans.first() {
                    first_span_leading_pause = first_span.leading_pause;
                }
                mitigated_payload = crate::prosody_payload::encode_spans(spans);
            }
            
            let mut audio = actor.render(mitigated_payload.clone());
            
            if first_span_leading_pause > 0.0 {
                let silence_len = (first_span_leading_pause * sample_rate as f64) as usize;
                let mut silence = vec![0.0f32; silence_len];
                silence.extend(audio);
                audio = silence;
            }
            
            // Re-lock to push the chunk
            let mut state_guard = lock.lock().unwrap();
            if state_guard.stopped {
                break;
            }
            
            let index = state_guard.produced;
            state_guard.produced += 1;
            
            let chunk = AudioChunk {
                passage,
                payload: mitigated_payload,
                audio,
                sample_rate,
                index,
            };
            
            state_guard.pre_rendered_queue.push_back(chunk);
            cvar.notify_all(); // Wake up next_chunk if it is waiting
        }
    };
    
    run_loop();
    
    // Always notify waiting threads when worker terminates to avoid deadlocks
    let mut state_guard = lock.lock().unwrap();
    state_guard.source_exhausted = true;
    cvar.notify_all();
}

/// Drives the narration pipeline one chunk at a time.
#[derive(uniffi::Object)]
pub struct StageCoordinator {
    source: Arc<dyn NarrationSource>,
    director: Arc<dyn DirectorInference>,
    actor: Arc<dyn VocalActor>,
    grouping: crate::segmenter::NarrationGrouping,
    sample_rate: u32,
    state: Arc<(Mutex<CoordinatorState>, Condvar)>,
    lookahead_limit: usize,
    worker_thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

#[uniffi::export]
impl StageCoordinator {
    #[uniffi::constructor]
    pub fn new(
        source: Box<dyn NarrationSource>,
        director: Box<dyn DirectorInference>,
        actor: Box<dyn VocalActor>,
        grouping: crate::segmenter::NarrationGrouping,
        sample_rate: u32,
    ) -> Arc<Self> {
        Self::new_with_lookahead(source, director, actor, grouping, sample_rate, 0)
    }

    #[uniffi::constructor]
    pub fn new_with_lookahead(
        source: Box<dyn NarrationSource>,
        director: Box<dyn DirectorInference>,
        actor: Box<dyn VocalActor>,
        grouping: crate::segmenter::NarrationGrouping,
        sample_rate: u32,
        lookahead_limit: u32,
    ) -> Arc<Self> {
        let source: Arc<dyn NarrationSource> = Arc::from(source);
        let director: Arc<dyn DirectorInference> = Arc::from(director);
        let actor: Arc<dyn VocalActor> = Arc::from(actor);
        
        let state = Arc::new((Mutex::new(CoordinatorState::default()), Condvar::new()));
        let limit = lookahead_limit as usize;
        
        let worker_thread = if limit > 0 {
            let state_clone = state.clone();
            let source_clone = source.clone();
            let director_clone = director.clone();
            let actor_clone = actor.clone();
            let grouping_clone = grouping.clone();
            
            let handle = std::thread::spawn(move || {
                run_worker(
                    state_clone,
                    source_clone,
                    director_clone,
                    actor_clone,
                    grouping_clone,
                    sample_rate,
                    limit,
                );
            });
            Some(handle)
        } else {
            None
        };
        
        Arc::new(Self {
            source,
            director,
            actor,
            grouping,
            sample_rate,
            state,
            lookahead_limit: limit,
            worker_thread: Mutex::new(worker_thread),
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
        if self.lookahead_limit == 0 {
            // Original synchronous inline rendering
            let (lock, _) = &*self.state;
            let mut state = lock.lock().unwrap();
            if state.stopped {
                return None;
            }
            
            while state.chunk_buffer.is_empty() && !state.source_exhausted {
                drop(state);
                let raw_passage = self.source.next_passage();
                state = lock.lock().unwrap();
                if state.stopped {
                    return None;
                }
                match raw_passage {
                    Some(passage) => {
                        let sentences = crate::segmenter::split_sentences(&passage);
                        let grouped = self.grouping.group(&sentences);
                        state.chunk_buffer.extend(grouped);
                    }
                    None => {
                        state.source_exhausted = true;
                    }
                }
            }
            
            let passage = state.chunk_buffer.pop_front()?;
            drop(state);
            
            let payload = self.director.annotate(passage.clone());
            
            let mut mitigated_payload = payload.clone();
            let mut first_span_leading_pause = 0.0;
            if let Some(decoded) = crate::prosody_payload::decode_spans(&payload) {
                let mut spans = decoded.spans;
                spans = crate::segmenter::apply_boundary_mitigations(spans);
                
                let mut state = lock.lock().unwrap();
                if state.stopped {
                    return None;
                }
                let prev_speaker = state.last_speaker.clone();
                
                if !spans.is_empty() {
                    let first_speaker = spans[0].acoustics.as_ref().and_then(|a| a.speaker_lock.clone());
                    if prev_speaker != first_speaker {
                        if state.emitted > 0 {
                            spans[0].leading_pause = spans[0].leading_pause.max(0.25);
                        }
                    }
                    state.last_speaker = spans.last().and_then(|s| s.acoustics.as_ref().and_then(|a| a.speaker_lock.clone()));
                }
                drop(state);
                
                if let Some(first_span) = spans.first() {
                    first_span_leading_pause = first_span.leading_pause;
                }
                mitigated_payload = crate::prosody_payload::encode_spans(spans);
            }
            
            let mut audio = self.actor.render(mitigated_payload.clone());
            
            if first_span_leading_pause > 0.0 {
                let silence_len = (first_span_leading_pause * self.sample_rate as f64) as usize;
                let mut silence = vec![0.0f32; silence_len];
                silence.extend(audio);
                audio = silence;
            }
            
            let mut state = lock.lock().unwrap();
            if state.stopped {
                return None;
            }
            let index = state.emitted;
            state.emitted += 1;
            
            Some(AudioChunk {
                passage,
                payload: mitigated_payload,
                audio,
                sample_rate: self.sample_rate,
                index,
            })
        } else {
            // Bounded pre-rendered queue consumption
            let (lock, cvar) = &*self.state;
            let mut state = lock.lock().unwrap();
            
            while state.pre_rendered_queue.is_empty()
                && !state.stopped
                && !(state.source_exhausted && state.chunk_buffer.is_empty())
            {
                state = cvar.wait(state).unwrap();
            }
            
            if state.stopped {
                return None;
            }
            
            if let Some(chunk) = state.pre_rendered_queue.pop_front() {
                state.emitted += 1;
                cvar.notify_all(); // Wake up worker to pre-render next chunks
                Some(chunk)
            } else {
                None
            }
        }
    }

    /// Stop the session. Subsequent calls to [`next_chunk`] return `None`.
    ///
    /// [`next_chunk`]: StageCoordinator::next_chunk
    pub fn stop(&self) {
        let (lock, cvar) = &*self.state;
        let mut state = lock.lock().unwrap();
        state.stopped = true;
        cvar.notify_all();
    }

    /// Number of chunks emitted so far.
    pub fn chunks_emitted(&self) -> u32 {
        let (lock, _) = &*self.state;
        lock.lock().unwrap().emitted
    }
}

impl Drop for StageCoordinator {
    fn drop(&mut self) {
        self.stop();
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
        StageCoordinator::new_with_lookahead(
            Box::new(VecSource::new(items)),
            Box::new(EchoDirector),
            Box::new(LenActor { calls }),
            crate::segmenter::NarrationGrouping::Sentence,
            24_000,
            0, // lookahead_limit of 0 runs original synchronous inline rendering tests
        )
    }

    #[test]
    fn emits_chunks_in_order_then_none() {
        let calls = Arc::new(AtomicUsize::new(0));
        let coord = coordinator(&["one", "two"], calls.clone());

        let c0 = coord.next_chunk().unwrap();
        assert_eq!(c0.index, 0);
        assert_eq!(c0.passage, "one");
        assert_eq!(c0.payload, "[V: 0.00 A: 0.00 T: 0.00] one");
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

    #[test]
    fn test_lookahead_rendering() {
        let calls = Arc::new(AtomicUsize::new(0));
        let coord = StageCoordinator::new_with_lookahead(
            Box::new(VecSource::new(&["one", "two", "three", "four"])),
            Box::new(EchoDirector),
            Box::new(LenActor { calls: calls.clone() }),
            crate::segmenter::NarrationGrouping::Sentence,
            24_000,
            2, // lookahead limit of 2
        );

        let wait_for_queue_len = |target_len: usize| {
            let (lock, cvar) = &*coord.state;
            let mut state = lock.lock().unwrap();
            while state.pre_rendered_queue.len() < target_len && !state.source_exhausted {
                state = cvar.wait(state).unwrap();
            }
        };

        // Wait until the background thread has pre-rendered up to the limit (2)
        wait_for_queue_len(2);
        
        // Since limit is 2, it should have rendered exactly 2 items.
        assert_eq!(calls.load(Ordering::SeqCst), 2);

        // Pull the first chunk
        let c0 = coord.next_chunk().unwrap();
        assert_eq!(c0.index, 0);
        assert_eq!(c0.passage, "one");

        // After pulling 1 chunk, the queue has space, so the background thread should pre-render the 3rd item
        // Wait until queue length becomes 2 again
        wait_for_queue_len(2);
        assert_eq!(calls.load(Ordering::SeqCst), 3);

        // Pull the rest
        let c1 = coord.next_chunk().unwrap();
        assert_eq!(c1.index, 1);
        let c2 = coord.next_chunk().unwrap();
        assert_eq!(c2.index, 2);
        let c3 = coord.next_chunk().unwrap();
        assert_eq!(c3.index, 3);

        assert!(coord.next_chunk().is_none());
        assert_eq!(coord.chunks_emitted(), 4);
        assert_eq!(calls.load(Ordering::SeqCst), 4);
    }
}
