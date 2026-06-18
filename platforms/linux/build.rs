fn main() {
    if cfg!(feature = "alsa") && cfg!(feature = "pulse") {
        panic!("Both 'alsa' and 'pulse' features cannot be enabled simultaneously. Please choose one.");
    }

    #[cfg(target_os = "linux")]
    {
        let mut builder = cc::Build::new();
        builder.include("src");
        
        if cfg!(feature = "alsa") {
            builder.file("src/audio_sink_alsa.c");
            println!("cargo:rustc-link-lib=asound");
        } else if cfg!(feature = "pulse") {
            builder.file("src/audio_sink_pulse.c");
            println!("cargo:rustc-link-lib=pulse-simple");
            println!("cargo:rustc-link-lib=pulse");
        }
        builder.compile("audio_sink");
    }
}
