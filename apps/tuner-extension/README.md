# ProsodiaTunerExtension Chrome Extension 🎛️⚡

Have you ever wanted to dial in the exact coordinates of human emotion using dial knobs and range sliders? Welcome to **ProsodiaTunerExtension**, an interactive acoustic emotion tuner, real-time audio visualizer, and dataset collection companion for the **Project Prosodia** speech synthesis engine.

Designed as a modern, cyberpunk-inspired glassmorphic interface, this extension lets developers and voice tuners audition texts, modulate acoustic vector knobs, analyze pitch metrics, and build fine-tuning datasets live from the web (e.g. YouTube, Vimeo, or generic reading pages).

---

## ✨ Features (Or: Your new dashboard of feelings)

### 🎚️ 1. Affective & Acoustic Vector Tuning
- **Affective Space Sliders**: Fine-tune **Valence**, **Arousal**, and **Tension** vector parameters live. Watch your coordinates map onto voice styles instantly.
- **Acoustic Modulation Knobs**: Customize **Speed Bias**, **Gain (Volume) Bias**, and **Pause Duration Multipliers**. Play director from the comfort of your browser.
- **Engine Rules**: Set specific **Speaker Lock IDs** and **Pronunciation/Phoneme Overrides** directly.
- **Live Code Preview**: Generates raw downstream annotation payloads (e.g. `[V: 0.85 A: -0.20 T: 0.10 SB: 1.10] Enter text...`) in real-time. Click to copy and paste straight into your narration tracks!

### 🎙️ 2. Real-Time Mic Spectrogram & Analytics
- **Live Visualizer**: Captures local microphone streams and renders a neon glowing **Spectrogram Canvas**. Whistle, talk, or sing, and watch the waves dance.
- **Acoustic Metrics**: Computes and displays real-time **RMS (Root Mean Square)** for loudness and **Pitch (Hz)** using a robust autocorrelation pitch detection algorithm. Highly scientific, but also incredibly hypnotic to watch.

### 📁 3. Direct Native File System Logging
- **Persistent Local Link**: Uses the modern **File System Access API** (with IndexedDB state persistence) to let you link a local JSON dataset file (`prosodia_tuning_dataset.json`).
- **Direct Appends**: Safely logs segments directly to the linked local file without requiring server upload/download cycles. Your data never leaves your computer!
- **Smart Safety Net**: Built-in **4-second floating Toast Overlay with progress countdown** allowing you to **Undo** any logged action in case you clicked log while sneezing or coughing.
- **Export Utility**: Instantly downloads your local session dataset as a formatted JSON document.

### 🔗 4. Smart Metadata Context Sync
- **One-click Refresh**: Queries active tabs (with specialized selectors for platforms like **YouTube**) to auto-extract video titles, current timestamps, and any **selected/highlighted webpage text** to seed your tuning context instantly.

---

## 🛠️ Installation (Developer Unpacked Mode)

Since this extension is in active development, load it as an unpacked extension:

1. Open Google Chrome and navigate to **`chrome://extensions/`**.
2. Toggle **Developer mode** **ON** in the top-right corner.
3. Click the **Load unpacked** button in the top-left corner.
4. Select the project directory:
    📁 `~/Projects/Prosodia/ProsodiaTunerExtension`

---

## 📂 File Structure

- [manifest.json](manifest.json) – Configuration manifest (Manifest V3) declaring storage, tab active scripting permissions, and the root icons mapping.
- [popup.html](popup.html) – The structural layout of the Tuner, Visualizer, and Session list views.
- [popup.css](popup.css) – Visual design system including glassmorphism sheets, neon glows, variables, and animations.
- [popup.js](popup.js) – Core interactive logic, File System Access API linkages, spectrogram rendering, and IndexedDB state handlers.
- [content.js](content.js) – Content script executing inside tabs to harvest highlighted text and playback time.

---

## 📄 License
Licensed under the Apache 2.0 License. See the `LICENSE` file for details.
