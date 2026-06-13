document.addEventListener("DOMContentLoaded", () => {
  // --- UI Elements ---
  const tabButtons = document.querySelectorAll(".tab-btn");
  const tabPanes = document.querySelectorAll(".tab-pane");
  const datasetCountBadge = document.getElementById("dataset-count");

  // Storage / File Linking Elements
  const btnLinkFile = document.getElementById("link-file-btn");
  const fileStatusText = document.getElementById("file-status");
  const fileStatusDot = document.getElementById("file-dot");

  // Tuner Inputs
  const inputSource = document.getElementById("clip-source");
  const inputTimeStart = document.getElementById("clip-time-start");
  const inputTimeEnd = document.getElementById("clip-time-end");

  // Dials & Sliders
  const inputEmotionLabel = document.getElementById("emotion-label");
  const sliderValence = document.getElementById("slider-valence");
  const sliderArousal = document.getElementById("slider-arousal");
  const sliderTension = document.getElementById("slider-tension");
  const sliderSpeedBias = document.getElementById("slider-speed-bias");
  const sliderGainBias = document.getElementById("slider-gain-bias");
  const sliderPauseMult = document.getElementById("slider-pause-mult");

  // Value Badges
  const valValence = document.getElementById("val-valence");
  const valArousal = document.getElementById("val-arousal");
  const valTension = document.getElementById("val-tension");
  const valSpeedBias = document.getElementById("val-speed-bias");
  const valGainBias = document.getElementById("val-gain-bias");
  const valPauseMult = document.getElementById("val-pause-mult");

  // Preview & Actions
  const codePreview = document.getElementById("code-preview-block");
  const btnCopyCode = document.getElementById("copy-code-btn");
  const btnLogSegment = document.getElementById("log-segment-btn");
  const btnDiscard = document.getElementById("discard-btn");

  // Floating Undo Toast Elements
  const undoToast = document.getElementById("undo-toast");
  const btnUndo = document.getElementById("undo-btn");
  const toastProgressBar = document.getElementById("toast-progress-bar");
  const toastMessage = document.getElementById("toast-message");

  // Visualizer elements
  const btnMicToggle = document.getElementById("mic-toggle-btn");
  const visualizerContainer = document.querySelector(".visualizer-container");
  const canvas = document.getElementById("spectrogram-canvas");
  const canvasCtx = canvas.getContext("2d");
  const metricRms = document.getElementById("metric-rms");
  const metricPitch = document.getElementById("metric-pitch");
  const autoTuneCheckbox = document.getElementById("auto-tune-checkbox");
  const speakerSelect = document.getElementById("speaker-select");
  const btnPopout = document.getElementById("popout-btn");

  // Dataset elements
  const btnExportJson = document.getElementById("export-json-btn");
  const btnClearData = document.getElementById("clear-data-btn");
  const emptyListMsg = document.getElementById("empty-list-msg");
  const loggedList = document.getElementById("logged-segments-list");

  // --- Local State ---
  let dataset = [];
  let fileHandle = null;
  
  // Undo Save State variables
  let saveTimeout = null;
  let pendingSegment = null;
  
  // Audio state variables
  let audioContext = null;
  let analyser = null;
  let microphoneStream = null;
  let animationFrameId = null;
  let samplesBuffer = [];
  let metricsIntervalId = null;
  let lastCaptureTimeSeries = [];

  // --- Tab Navigation ---
  tabButtons.forEach(btn => {
    btn.addEventListener("click", () => {
      const tabId = btn.getAttribute("data-tab");
      
      tabButtons.forEach(b => b.classList.remove("active"));
      tabPanes.forEach(pane => pane.classList.remove("active"));
      
      btn.classList.add("active");
      document.getElementById(`tab-${tabId}`).classList.add("active");
    });
  });

  // --- Pop-Out Standalone Window Handling ---
  const urlParams = new URLSearchParams(window.location.search);
  const isPopout = urlParams.get("window") === "true";

  if (isPopout) {
    if (btnPopout) {
      btnPopout.style.display = "none";
    }
    document.body.classList.add("popout-window");
  }

  if (btnPopout) {
    btnPopout.addEventListener("click", () => {
      chrome.windows.create({
        url: chrome.runtime.getURL("popup.html?window=true"),
        type: "popup",
        width: 390,
        height: 620
      }, () => {
        window.close();
      });
    });
  }

  // --- IndexedDB for File System Handles ---
  function openDB() {
    return new Promise((resolve, reject) => {
      const request = indexedDB.open("ProsodiaTunerExtensionDB", 1);
      request.onupgradeneeded = (e) => {
        const db = e.target.result;
        if (!db.objectStoreNames.contains("handles")) {
          db.createObjectStore("handles");
        }
      };
      request.onsuccess = (e) => resolve(e.target.result);
      request.onerror = (e) => reject(e.target.error);
    });
  }

  async function saveFileHandle(handle) {
    const db = await openDB();
    const tx = db.transaction("handles", "readwrite");
    tx.objectStore("handles").put(handle, "fileHandle");
    await new Promise(r => tx.oncomplete = r);
  }

  async function getStoredFileHandle() {
    const db = await openDB();
    const tx = db.transaction("handles", "readonly");
    const handle = await new Promise(r => {
      const req = tx.objectStore("handles").get("fileHandle");
      req.onsuccess = () => r(req.result);
    });
    return handle;
  }

  // --- File Connection Verifier ---
  async function verifyFileConnection(promptUser = false) {
    try {
      const handle = await getStoredFileHandle();
      if (!handle) {
        setFileStatus(null);
        return false;
      }
      
      fileHandle = handle;
      const opts = { mode: "readwrite" };
      let permission = await handle.queryPermission(opts);
      
      if (permission !== "granted" && promptUser) {
        permission = await handle.requestPermission(opts);
      }
      
      if (permission === "granted") {
        setFileStatus(handle.name);
        return true;
      } else {
        setFileStatus(handle.name, false); // Stored but disconnected
        return false;
      }
    } catch (err) {
      console.error("Failed to verify file connection:", err);
      setFileStatus(null);
      return false;
    }
  }

  function setFileStatus(fileName, isConnected = true) {
    if (fileName) {
      fileStatusText.textContent = fileName;
      if (isConnected) {
        fileStatusDot.className = "status-dot green";
        btnLinkFile.textContent = "Change File";
      } else {
        fileStatusDot.className = "status-dot orange";
        btnLinkFile.textContent = "Connect";
      }
    } else {
      fileStatusText.textContent = "No tuning file linked";
      fileStatusDot.className = "status-dot orange";
      btnLinkFile.textContent = "Link File";
    }
  }

  // --- Link Local File ---
  async function linkLocalTuningFile() {
    try {
      const opts = {
        suggestedName: "prosodia_tuning_dataset.json",
        types: [{
          description: "JSON Tuning Dataset",
          accept: { "application/json": [".json"] }
        }]
      };
      
      // Request file creation or selection from user
      const handle = await window.showSaveFilePicker(opts);
      fileHandle = handle;
      
      await saveFileHandle(handle);
      setFileStatus(handle.name);
      
      // Seed file with empty array if empty
      const file = await handle.getFile();
      const text = await file.text();
      if (!text.trim()) {
        const writable = await handle.createWritable();
        await writable.write("[]");
        await writable.close();
      }
    } catch (err) {
      if (err.name !== "AbortError") {
        console.error("Failed to link local file:", err);
        alert("Could not link file: " + err.message);
      }
    }
  }

  btnLinkFile.addEventListener("click", async () => {
    if (fileHandle) {
      const connected = await verifyFileConnection(false);
      if (connected) {
        // Already connected. Clicking allows picking a different file.
        linkLocalTuningFile();
      } else {
        // Try to connect (prompt permission). If failed/rejected, select new file.
        const reconnected = await verifyFileConnection(true);
        if (!reconnected) {
          linkLocalTuningFile();
        }
      }
    } else {
      linkLocalTuningFile();
    }
  });

  // --- Local In-Popup Session Storage ---
  function loadSessionDataset() {
    chrome.storage.local.get(["prosodiaSessionDataset"], (result) => {
      if (result.prosodiaSessionDataset) {
        dataset = result.prosodiaSessionDataset;
        updateSessionUI();
      }
    });
  }

  function saveSessionDataset() {
    chrome.storage.local.set({ prosodiaSessionDataset: dataset }, () => {
      updateSessionUI();
    });
  }

  // --- Format Webpage Time ---
  function formatSeconds(seconds) {
    if (isNaN(seconds) || seconds === null) return "0:00";
    const hrs = Math.floor(seconds / 3600);
    const mins = Math.floor((seconds % 3600) / 60);
    const secs = Math.floor(seconds % 60);
    
    const formattedSecs = secs < 10 ? `0${secs}` : secs;
    if (hrs > 0) {
      const formattedMins = mins < 10 ? `0${mins}` : mins;
      return `${hrs}:${formattedMins}:${formattedSecs}`;
    }
    return `${mins}:${formattedSecs}`;
  }

  // --- Get Target Tab for Syncing/Capture ---
  function getTargetTab(callback) {
    chrome.windows.getLastFocused({ populate: true, windowTypes: ["normal"] }, (win) => {
      if (win && win.tabs) {
        const activeTab = win.tabs.find(t => t.active);
        if (activeTab) {
          callback(activeTab);
          return;
        }
      }
      // Fallback to active tab in current context
      chrome.tabs.query({ active: true, currentWindow: true }, (tabs) => {
        if (tabs && tabs[0]) {
          callback(tabs[0]);
        } else {
          callback(null);
        }
      });
    });
  }

  // --- Get and Set Playhead Timestamps ---
  function syncCaptureStart() {
    getTargetTab((activeTab) => {
      if (!activeTab) {
        inputTimeStart.value = "0:00";
        return;
      }
      
      chrome.tabs.sendMessage(activeTab.id, { action: "getPageInfo" }, (response) => {
        if (chrome.runtime.lastError || !response) {
          inputSource.value = activeTab.title || "";
          inputTimeStart.value = "0:00";
          return;
        }
        
        inputSource.value = response.title || "";
        inputTimeStart.value = formatSeconds(response.currentTime);
        updatePreview();
      });
    });
  }

  function syncCaptureEnd() {
    getTargetTab((activeTab) => {
      if (!activeTab) return;
      
      chrome.tabs.sendMessage(activeTab.id, { action: "getPageInfo" }, (response) => {
        if (chrome.runtime.lastError || !response) return;
        
        inputTimeEnd.value = formatSeconds(response.currentTime);
        updatePreview();
      });
    });
  }

  // Auto-sync active source on startup
  getTargetTab((activeTab) => {
    if (activeTab) {
      inputSource.value = activeTab.title || "";
    }
  });

  // --- Format Affective Block ---
  function getAffectiveString() {
    const v = parseFloat(sliderValence.value).toFixed(2);
    const a = parseFloat(sliderArousal.value).toFixed(2);
    const t = parseFloat(sliderTension.value).toFixed(2);
    
    let block = `[V: ${v} A: ${a} T: ${t}`;
    
    const sb = parseFloat(sliderSpeedBias.value);
    if (sb !== 0) {
      block += ` SB: ${sb.toFixed(3)}`;
    }
    
    const gb = parseFloat(sliderGainBias.value);
    if (gb !== 0) {
      block += ` GB: ${gb.toFixed(3)}`;
    }
    
    const pb = parseFloat(sliderPauseMult.value);
    if (pb !== 1.0) {
      block += ` PB: ${pb.toFixed(3)}`;
    }
    
    block += `]`;
    return block;
  }

  // --- Render Preview Block ---
  function updatePreview() {
    valValence.textContent = parseFloat(sliderValence.value).toFixed(2);
    valArousal.textContent = parseFloat(sliderArousal.value).toFixed(2);
    valTension.textContent = parseFloat(sliderTension.value).toFixed(2);
    valSpeedBias.textContent = parseFloat(sliderSpeedBias.value).toFixed(3);
    valGainBias.textContent = parseFloat(sliderGainBias.value).toFixed(3);
    valPauseMult.textContent = parseFloat(sliderPauseMult.value).toFixed(3);
    
    codePreview.textContent = getAffectiveString();
  }

  // Bind input change listeners
  [sliderValence, sliderArousal, sliderTension, sliderSpeedBias, sliderGainBias, sliderPauseMult].forEach(el => {
    el.addEventListener("input", updatePreview);
  });
  [inputEmotionLabel, inputTimeStart, inputTimeEnd].forEach(el => {
    el.addEventListener("input", updatePreview);
  });

  // --- Copy to Clipboard ---
  btnCopyCode.addEventListener("click", () => {
    const text = getAffectiveString();
    navigator.clipboard.writeText(text).then(() => {
      btnCopyCode.textContent = "Copied!";
      btnCopyCode.style.color = "var(--valence-color)";
      btnCopyCode.style.borderColor = "var(--valence-color)";
      setTimeout(() => {
        btnCopyCode.textContent = "Copy";
        btnCopyCode.style.color = "var(--text-secondary)";
        btnCopyCode.style.borderColor = "var(--border-glow)";
      }, 1500);
    });
  });

  // --- Audio Autocorrelation Pitch Detector ---
  function autoCorrelate(buffer, sampleRate) {
    const SIZE = buffer.length;
    let r = new Float32Array(SIZE);
    
    for (let i = 0; i < SIZE; i++) {
      for (let j = 0; j < SIZE - i; j++) {
        r[i] += buffer[j] * buffer[j + i];
      }
    }
    
    let d = 0;
    while (d < SIZE - 1 && r[d] > r[d + 1]) {
      d++;
    }
    
    let peak = -1;
    let maxVal = -1;
    for (let i = d; i < SIZE; i++) {
      if (r[i] > maxVal) {
        maxVal = r[i];
        peak = i;
      }
    }
    
    if (peak !== -1 && maxVal > 0.01) {
      return sampleRate / peak;
    }
    return -1;
  }

  // --- Visualizer Render Loop ---
  function drawVisualizer() {
    if (!analyser) return;
    
    const bufferLength = analyser.fftSize;
    const dataArray = new Float32Array(bufferLength);
    analyser.getFloatTimeDomainData(dataArray);
    
    const width = canvas.width = visualizerContainer.clientWidth - 16;
    const height = canvas.height = 50;
    
    canvasCtx.fillStyle = "#040508";
    canvasCtx.fillRect(0, 0, width, height);
    
    canvasCtx.lineWidth = 2;
    canvasCtx.strokeStyle = "rgba(0, 242, 254, 0.85)";
    canvasCtx.shadowBlur = 8;
    canvasCtx.shadowColor = "var(--valence-color)";
    canvasCtx.beginPath();
    
    const sliceWidth = width / bufferLength;
    let x = 0;
    
    let sumSquares = 0;
    for (let i = 0; i < bufferLength; i++) {
      const v = dataArray[i];
      sumSquares += v * v;
      
      const y = (v + 1) * height / 2;
      
      if (i === 0) {
        canvasCtx.moveTo(x, y);
      } else {
        canvasCtx.lineTo(x, y);
      }
      x += sliceWidth;
    }
    
    canvasCtx.lineTo(width, height / 2);
    canvasCtx.stroke();
    canvasCtx.shadowBlur = 0;
    
    const rms = Math.sqrt(sumSquares / bufferLength);
    metricRms.textContent = rms.toFixed(3);
    
    const pitch = autoCorrelate(dataArray, audioContext.sampleRate);
    if (pitch !== -1 && pitch >= 50 && pitch <= 1200) {
      metricPitch.textContent = `${Math.round(pitch)} Hz`;
    } else {
      metricPitch.textContent = "--- Hz";
    }
    
    // Auto-tune dials based on live volume (RMS) and frequency (Pitch)
    if (autoTuneCheckbox && autoTuneCheckbox.checked) {
      let arousalTarget = 0.0;
      let gainTarget = 0.0;
      let tensionTarget = 0.0;
      let speedTarget = 0.0;

      if (rms > 0.002) {
        arousalTarget = -1.0 + ((rms - 0.002) / (0.12 - 0.002)) * 2.0;
        arousalTarget = Math.max(-1.0, Math.min(1.0, arousalTarget));

        gainTarget = -0.4 + ((rms - 0.002) / (0.12 - 0.002)) * 0.8;
        gainTarget = Math.max(-0.4, Math.min(0.4, gainTarget));
      }

      if (pitch !== -1 && pitch >= 50 && pitch <= 600) {
        tensionTarget = (pitch - 90) / (300 - 90);
        tensionTarget = Math.max(0.0, Math.min(1.0, tensionTarget));

        speedTarget = -0.4 + ((pitch - 90) / (300 - 90)) * 0.8;
        speedTarget = Math.max(-0.4, Math.min(0.4, speedTarget));
      }

      // Exponential smoothing (alpha = 0.1)
      const currentArousal = parseFloat(sliderArousal.value);
      sliderArousal.value = (0.1 * arousalTarget + 0.9 * currentArousal).toFixed(2);

      const currentGain = parseFloat(sliderGainBias.value);
      sliderGainBias.value = (0.1 * gainTarget + 0.9 * currentGain).toFixed(3);

      const currentTension = parseFloat(sliderTension.value);
      sliderTension.value = (0.1 * tensionTarget + 0.9 * currentTension).toFixed(2);

      const currentSpeed = parseFloat(sliderSpeedBias.value);
      sliderSpeedBias.value = (0.1 * speedTarget + 0.9 * currentSpeed).toFixed(3);

      updatePreview();
    }
    
    animationFrameId = requestAnimationFrame(drawVisualizer);
  }

  // --- Enumerate & Populate Speaker Outputs ---
  async function updateOutputDevicesList() {
    if (!speakerSelect) return;
    try {
      const devices = await navigator.mediaDevices.enumerateDevices();
      const audioOutputs = devices.filter(device => device.kind === "audiooutput");
      
      const currentValue = speakerSelect.value;
      speakerSelect.innerHTML = '<option value="">Default Speaker</option>';
      
      audioOutputs.forEach((device, index) => {
        const label = device.label || `Speaker ${index + 1}`;
        const option = document.createElement("option");
        option.value = device.deviceId;
        option.textContent = label;
        speakerSelect.appendChild(option);
      });
      
      // Preserve current selection if it still exists
      if (currentValue && audioOutputs.some(d => d.deviceId === currentValue)) {
        speakerSelect.value = currentValue;
      }
    } catch (err) {
      console.error("Failed to enumerate audio output devices: ", err);
    }
  }

  // --- Calculate VAD/Bias Metrics Averages per Second ---
  function getPerSecondAverages() {
    const timeSeries = [];
    const samplesPerSecond = 10; // 100ms sample interval
    
    for (let i = 0; i < samplesBuffer.length; i += samplesPerSecond) {
      const chunk = samplesBuffer.slice(i, i + samplesPerSecond);
      if (chunk.length === 0) continue;
      
      let sumValence = 0, sumArousal = 0, sumTension = 0, sumSpeed = 0, sumGain = 0;
      chunk.forEach(s => {
        sumValence += s.valence;
        sumArousal += s.arousal;
        sumTension += s.tension;
        sumSpeed += s.speedBias;
        sumGain += s.gainBias;
      });
      
      const count = chunk.length;
      const secondNum = Math.floor(i / samplesPerSecond) + 1;
      
      timeSeries.push({
        second: secondNum,
        valence: parseFloat((sumValence / count).toFixed(2)),
        arousal: parseFloat((sumArousal / count).toFixed(2)),
        tension: parseFloat((sumTension / count).toFixed(2)),
        speedBias: parseFloat((sumSpeed / count).toFixed(3)),
        gainBias: parseFloat((sumGain / count).toFixed(3))
      });
    }
    return timeSeries;
  }

  // --- Toggle Tab Audio Capture ---
  btnMicToggle.addEventListener("click", async () => {
    if (audioContext) {
      // Clear metric sampling interval
      if (metricsIntervalId) {
        clearInterval(metricsIntervalId);
        metricsIntervalId = null;
      }
      
      // Stamp playhead end time
      syncCaptureEnd();
      
      // Compute averages
      lastCaptureTimeSeries = getPerSecondAverages();
      console.log("Captured time-series averages:", lastCaptureTimeSeries);

      cancelAnimationFrame(animationFrameId);
      if (microphoneStream) {
        microphoneStream.getTracks().forEach(track => track.stop());
      }
      if (audioContext.state !== "closed") {
        await audioContext.close();
      }
      
      audioContext = null;
      analyser = null;
      microphoneStream = null;
      
      btnMicToggle.textContent = "Capture Tab Audio";
      visualizerContainer.classList.add("hide");
    } else {
      getTargetTab(async (activeTab) => {
        if (!activeTab) {
          alert("No active browser tab found to capture audio from.");
          return;
        }

        try {
          chrome.tabCapture.getMediaStreamId({ targetTabId: activeTab.id }, async (streamId) => {
            if (chrome.runtime.lastError || !streamId) {
              const errMsg = chrome.runtime.lastError ? chrome.runtime.lastError.message : "Could not obtain stream ID";
              console.error("tabCapture.getMediaStreamId error:", errMsg);
              alert("Failed to capture tab audio: " + errMsg);
              return;
            }

            try {
              const stream = await navigator.mediaDevices.getUserMedia({
                audio: {
                  mandatory: {
                    chromeMediaSource: "tab",
                    chromeMediaSourceId: streamId
                  }
                },
                video: false
              });

              microphoneStream = stream; // keep variable name consistent with visualizer draw references
              
              audioContext = new (window.AudioContext || window.webkitAudioContext)();
              analyser = audioContext.createAnalyser();
              analyser.fftSize = 2048;
              
              const source = audioContext.createMediaStreamSource(stream);
              source.connect(analyser);
              
              // Route output back to user destination so they can hear it
              analyser.connect(audioContext.destination);
              
              // Apply selected speaker if any
              const selectedSpeakerId = speakerSelect ? speakerSelect.value : "";
              if (selectedSpeakerId && typeof audioContext.setSinkId === "function") {
                await audioContext.setSinkId(selectedSpeakerId);
              }
              
              // Clear previous buffers and begin metrics logging interval
              samplesBuffer = [];
              lastCaptureTimeSeries = [];
              captureStartTime = Date.now();
              
              // Stamp playhead start time
              syncCaptureStart();
              
              metricsIntervalId = setInterval(() => {
                samplesBuffer.push({
                  valence: parseFloat(sliderValence.value),
                  arousal: parseFloat(sliderArousal.value),
                  tension: parseFloat(sliderTension.value),
                  speedBias: parseFloat(sliderSpeedBias.value),
                  gainBias: parseFloat(sliderGainBias.value)
                });
              }, 100);

              btnMicToggle.textContent = "Stop Capture";
              visualizerContainer.classList.remove("hide");
              drawVisualizer();
              
              // Populate speaker devices
              await updateOutputDevicesList();
            } catch (err) {
              console.error("getUserMedia error for tab capture:", err);
              btnMicToggle.textContent = "Capture Error: " + err.name;
            }
          });
        } catch (err) {
          console.error("Failed to initiate tab audio capture:", err);
          btnMicToggle.textContent = "Capture Error: " + err.name;
        }
      });
    }
  });

  // --- Live Speaker Output Device Swapping ---
  if (speakerSelect) {
    speakerSelect.addEventListener("change", async () => {
      const selectedSpeakerId = speakerSelect.value;
      if (audioContext && typeof audioContext.setSinkId === "function") {
        try {
          await audioContext.setSinkId(selectedSpeakerId);
          console.log(`Audio output successfully routed to speaker: ${selectedSpeakerId || "default"}`);
        } catch (err) {
          console.error("Failed to set audio output sink ID:", err);
          alert("Failed to route speaker: " + err.message);
        }
      }
    });
  }

  // --- File Writing Append Logic ---
  async function appendSegmentToDisk(segment) {
    if (!fileHandle) return;
    
    try {
      const file = await fileHandle.getFile();
      let fileData = [];
      try {
        const text = await file.text();
        if (text.trim()) {
          fileData = JSON.parse(text);
          if (!Array.isArray(fileData)) fileData = [];
        }
      } catch (e) {
        console.log("File is empty or malformed; initializing fresh array.");
      }
      
      // Format schema to align with downstream Project Prosodia engine requirements
      const entry = {
        emotion: {
          valence: segment.valence,
          arousal: segment.arousal,
          tension: segment.tension,
          label: segment.emotionLabel || undefined
        },
        acoustics: {
          speedBias: segment.speedBias !== 0 ? segment.speedBias : undefined,
          gainBias: segment.gainBias !== 0 ? segment.gainBias : undefined,
          pauseMultiplier: segment.pauseMultiplier !== 1.0 ? segment.pauseMultiplier : undefined
        },
        metadata: {
          source: segment.source,
          timestamp: segment.timestamp,
          formattedPayload: segment.wirePayload,
          timeSeries: segment.timeSeries && segment.timeSeries.length > 0 ? segment.timeSeries : undefined
        }
      };
      
      fileData.push(entry);
      
      // Open writable stream and write back
      const writable = await fileHandle.createWritable();
      await writable.write(JSON.stringify(fileData, null, 2));
      await writable.close();
      
      // Update UI feedback
      fileStatusText.textContent = `${fileHandle.name} (appended!)`;
      setTimeout(() => {
        if (fileHandle) fileStatusText.textContent = fileHandle.name;
      }, 2000);
      
    } catch (err) {
      console.error("Failed to append segment to file:", err);
      alert("Error saving directly to file: " + err.message);
    }
  }

  // --- Hide Floating Toast Overlay ---
  function hideToast() {
    undoToast.classList.add("hide");
    toastProgressBar.style.animation = "none";
    void toastProgressBar.offsetWidth; // Trigger reflow to clear animation state
  }

  // --- Show Floating Toast and Start Countdown ---
  function triggerSaveSequence(segment) {
    // Stop any existing timeout
    if (saveTimeout) {
      clearTimeout(saveTimeout);
    }
    
    pendingSegment = segment;
    
    // Set message
    const targetFile = fileHandle ? fileHandle.name : "Session View";
    toastMessage.innerHTML = `Logged! Appending to <strong>${targetFile}</strong> in 4s...`;
    
    // Setup and trigger progress bar animation
    toastProgressBar.style.animation = "none";
    void toastProgressBar.offsetWidth;
    toastProgressBar.style.animation = "shrink 4s linear forwards";
    
    // Show toast
    undoToast.classList.remove("hide");
    
    // Set timeout to commit the save
    saveTimeout = setTimeout(async () => {
      if (pendingSegment) {
        // 1. Log locally to POPUP UI view
        dataset.unshift(pendingSegment);
        saveSessionDataset();
        
        // 2. Log directly to linked file
        if (fileHandle) {
          const connected = await verifyFileConnection(false);
          if (connected) {
            await appendSegmentToDisk(pendingSegment);
          } else {
            console.warn("File was disconnected during save sequence. Segment logged in session view only.");
          }
        }
        
        pendingSegment = null;
        hideToast();
      }
    }, 4000);
  }

  // --- Cancel/Undo Log Event ---
  btnUndo.addEventListener("click", () => {
    if (saveTimeout) {
      clearTimeout(saveTimeout);
      saveTimeout = null;
    }
    
    if (pendingSegment) {
      // Re-populate emotion labels and playhead values
      inputEmotionLabel.value = pendingSegment.emotionLabel || "";
      
      const startEnd = pendingSegment.timestamp.split(" - ");
      inputTimeStart.value = startEnd[0] || "0:00";
      inputTimeEnd.value = startEnd[1] || "";
      
      // Update values
      sliderValence.value = pendingSegment.valence;
      sliderArousal.value = pendingSegment.arousal;
      sliderTension.value = pendingSegment.tension;
      sliderSpeedBias.value = pendingSegment.speedBias;
      sliderGainBias.value = pendingSegment.gainBias;
      sliderPauseMult.value = pendingSegment.pauseMultiplier;
      
      updatePreview();
      pendingSegment = null;
    }
    
    hideToast();
  });

  // --- Log & Append Segment Click Action ---
  btnLogSegment.addEventListener("click", async () => {
    // Ensure file connection is verified (requesting permission if disconnected).
    // Because this runs as the first statement in the click event, the user gesture is active!
    if (fileHandle) {
      const connected = await verifyFileConnection(true);
      if (!connected) {
        alert("Please connect the linked dataset file to save entries.");
        return;
      }
    } else {
      // Prompt user to link a file if they haven't yet, but allow proceeding without it
      const option = confirm("No local tuning file is linked yet. Log to session view only? (You can link a file at the top to auto-append).");
      if (!option) {
        btnLinkFile.style.boxShadow = "0 0 14px #a855f7";
        setTimeout(() => {
          btnLinkFile.style.boxShadow = "none";
        }, 2000);
        return;
      }
    }
    
    const startVal = inputTimeStart.value.trim() || "0:00";
    const endVal = inputTimeEnd.value.trim();
    const rangeTimestamp = endVal ? `${startVal} - ${endVal}` : startVal;

    const segment = {
      id: Date.now().toString(),
      source: inputSource.value.trim() || "Unknown Source",
      timestamp: rangeTimestamp,
      emotionLabel: inputEmotionLabel.value.trim() || null,
      valence: parseFloat(sliderValence.value),
      arousal: parseFloat(sliderArousal.value),
      tension: parseFloat(sliderTension.value),
      speedBias: parseFloat(sliderSpeedBias.value),
      gainBias: parseFloat(sliderGainBias.value),
      pauseMultiplier: parseFloat(sliderPauseMult.value),
      wirePayload: getAffectiveString(),
      timeSeries: lastCaptureTimeSeries
    };
    
    // Clear playhead end time so next capture starts fresh
    inputTimeEnd.value = "";
    updatePreview();
    
    // Trigger save countdown (with Undo option)
    triggerSaveSequence(segment);
  });

  // --- Discard Current Capture Action ---
  if (btnDiscard) {
    btnDiscard.addEventListener("click", () => {
      // Reset playhead inputs
      inputTimeStart.value = "0:00";
      inputTimeEnd.value = "";
      
      // Clear buffers
      lastCaptureTimeSeries = [];
      samplesBuffer = [];
      
      // Reset label
      inputEmotionLabel.value = "";
      
      // Reset dials and overrides to default values
      sliderValence.value = "0.00";
      sliderArousal.value = "0.00";
      sliderTension.value = "0.00";
      sliderSpeedBias.value = "0.000";
      sliderGainBias.value = "0.000";
      sliderPauseMult.value = "1.000";
      
      updatePreview();
      
      // Visual feedback: brief color flash on button
      btnDiscard.style.borderColor = "var(--tension-color)";
      setTimeout(() => {
        btnDiscard.style.borderColor = "rgba(255, 255, 255, 0.06)";
      }, 500);
    });
  }

  // --- Render Dataset View ---
  function updateSessionUI() {
    datasetCountBadge.textContent = dataset.length;
    
    if (dataset.length === 0) {
      emptyListMsg.style.display = "flex";
      loggedList.innerHTML = "";
      return;
    }
    
    emptyListMsg.style.display = "none";
    loggedList.innerHTML = "";
    
    dataset.forEach(item => {
      const card = document.createElement("div");
      card.className = "segment-item";
      card.setAttribute("data-id", item.id);
      
      const labelBadge = item.emotionLabel ? `<span class="val-badge" style="color: var(--arousal-color); font-size: 8.5px; border-color: rgba(255, 140, 0, 0.25); text-transform: capitalize; padding: 1px 4px;">${item.emotionLabel}</span>` : "";
      
      card.innerHTML = `
        <div class="item-meta">
          <span class="source">${item.source}</span>
          <div>
            ${labelBadge}
            <span class="time">${item.timestamp}</span>
          </div>
        </div>
        <div class="item-payload">${item.wirePayload}</div>
        <button class="item-delete" title="Remove from View">✕</button>
      `;
      
      // Bind delete handler
      card.querySelector(".item-delete").addEventListener("click", () => {
        dataset = dataset.filter(d => d.id !== item.id);
        saveSessionDataset();
      });
      
      loggedList.appendChild(card);
    });
  }

  // --- Clear Dataset List View ---
  btnClearData.addEventListener("click", () => {
    if (confirm("Clear local session view? This will NOT delete segments already committed directly to your file on disk.")) {
      dataset = [];
      saveSessionDataset();
    }
  });

  // --- Export Session JSON ---
  btnExportJson.addEventListener("click", () => {
    if (dataset.length === 0) return;
    
    const formattedData = dataset.map(item => ({
      emotion: {
        valence: item.valence,
        arousal: item.arousal,
        tension: item.tension,
        label: item.emotionLabel || undefined
      },
      acoustics: {
        speedBias: item.speedBias !== 0 ? item.speedBias : undefined,
        gainBias: item.gainBias !== 0 ? item.gainBias : undefined,
        pauseMultiplier: item.pauseMultiplier !== 1.0 ? item.pauseMultiplier : undefined
      },
      metadata: {
        source: item.source,
        timestamp: item.timestamp,
        formattedPayload: item.wirePayload,
        timeSeries: item.timeSeries && item.timeSeries.length > 0 ? item.timeSeries : undefined
      }
    }));

    const jsonString = JSON.stringify(formattedData, null, 2);
    const blob = new Blob([jsonString], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    
    const a = document.createElement("a");
    a.href = url;
    a.download = "prosodia_session_tuning.json";
    document.body.appendChild(a);
    a.click();
    
    document.body.removeChild(a);
    URL.revokeObjectURL(url);
  });

  // --- Start Connection Verification on Startup ---
  verifyFileConnection(false);
  
  // --- Initial Load ---
  loadSessionDataset();
  updatePreview();
});
