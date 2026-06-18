#[cfg(target_os = "linux")]
extern "C" {
    fn audio_sink_create(sample_rate: u32, channels: u32) -> *mut std::ffi::c_void;
    fn audio_sink_write(sink: *mut std::ffi::c_void, samples: *const f32, count: u32) -> i32;
    fn audio_sink_destroy(sink: *mut std::ffi::c_void);
}

pub struct LinuxAudioSink {
    sink: std::sync::Mutex<*mut std::ffi::c_void>,
}

unsafe impl Send for LinuxAudioSink {}
unsafe impl Sync for LinuxAudioSink {}

impl LinuxAudioSink {
    pub fn new(sample_rate: u32, channels: u32) -> Self {
        #[cfg(target_os = "linux")]
        {
            let sink = unsafe { audio_sink_create(sample_rate, channels) };
            assert!(!sink.is_null(), "Failed to create C audio sink");
            Self {
                sink: std::sync::Mutex::new(sink),
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (sample_rate, channels);
            Self {
                sink: std::sync::Mutex::new(std::ptr::null_mut()),
            }
        }
    }
}

impl Drop for LinuxAudioSink {
    fn drop(&mut self) {
        #[cfg(target_os = "linux")]
        {
            let sink = *self.sink.lock().unwrap();
            if !sink.is_null() {
                unsafe { audio_sink_destroy(sink) };
            }
        }
    }
}

impl actor::engine::AudioSink for LinuxAudioSink {
    fn schedule_audio(&self, audio: Vec<f32>, _sample_rate: u32) {
        #[cfg(target_os = "linux")]
        {
            let sink = *self.sink.lock().unwrap();
            if !sink.is_null() {
                unsafe {
                    audio_sink_write(sink, audio.as_ptr(), audio.len() as u32);
                }
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (audio, _sample_rate);
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
