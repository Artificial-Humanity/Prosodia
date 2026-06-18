#[cfg(target_os = "linux")]
extern "C" {
    fn audio_sink_create(sample_rate: u32, channels: u32) -> *mut std::ffi::c_void;
    fn audio_sink_write(sink: *mut std::ffi::c_void, samples: *const f32, count: u32) -> i32;
    fn audio_sink_destroy(sink: *mut std::ffi::c_void);
}

#[cfg(target_os = "linux")]
pub struct LinuxAudioSink {
    sink: std::sync::Mutex<*mut std::ffi::c_void>,
}

#[cfg(target_os = "linux")]
unsafe impl Send for LinuxAudioSink {}
#[cfg(target_os = "linux")]
unsafe impl Sync for LinuxAudioSink {}

#[cfg(target_os = "linux")]
impl LinuxAudioSink {
    pub fn new(sample_rate: u32, channels: u32) -> Self {
        let sink = unsafe { audio_sink_create(sample_rate, channels) };
        assert!(!sink.is_null(), "Failed to create C audio sink");
        Self {
            sink: std::sync::Mutex::new(sink),
        }
    }
}

#[cfg(target_os = "linux")]
impl Drop for LinuxAudioSink {
    fn drop(&mut self) {
        let sink = *self.sink.lock().unwrap();
        if !sink.is_null() {
            unsafe { audio_sink_destroy(sink) };
        }
    }
}

#[cfg(target_os = "linux")]
impl actor::engine::AudioSink for LinuxAudioSink {
    fn schedule_audio(&self, audio: Vec<f32>, _sample_rate: u32) {
        let sink = *self.sink.lock().unwrap();
        if !sink.is_null() {
            unsafe {
                audio_sink_write(sink, audio.as_ptr(), audio.len() as u32);
            }
        }
    }
}

fn main() {
    #[cfg(target_os = "linux")]
    {
        println!("Prosodia Linux Daemon started.");
        let sample_rate = stage::acoustic_matrix::get_sample_rate();
        let _sink = LinuxAudioSink::new(sample_rate, 1);
        println!("Initialized ALSA/PulseAudio sink at {}Hz.", sample_rate);
    }
    #[cfg(not(target_os = "linux"))]
    {
        println!("Prosodia Linux Daemon is only supported on Linux.");
    }
}
