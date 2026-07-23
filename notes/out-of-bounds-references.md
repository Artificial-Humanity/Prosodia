# Out-of-Bounds References Catalog

This document catalogs all cases in the `Prosodia` project where code, configuration, or tests reference paths outside the `Prosodia` project root. These out-of-bounds references typically point to the shared workspace `models/` link (-> `/data/models`; layout flattened 2026-07-22, paths updated 2026-07-23) or reference fallback paths in the developer's home directory.

---

## 🧭 Catalog of Occurrences

### 1. Crate Unit Tests (Actor Core)
* **File:** [crates/actor/src/engine.rs](file:///Users/lmcfarlin/Projects/Artificial-Humanity/Prosodia/crates/actor/src/engine.rs) (Lines 836, 867, 868, 976, 977, 1048, 1058, 1096)
* **Description:** Unit tests in the actor crate require loading raw TFLite model weights and configuration files. They navigate three levels up (`../../../models/...`) to read files from the shared `models/` directory:
  * `../../../models/matcha_stock.tflite`
  * `../../../models/sonora.tflite`
  * `../../../models/config.json`
* **Impact:** Running `cargo test` in a standalone checkout of the `Prosodia` repository will fail these tests unless a `models/` directory is manually populated at the parent directory level.

### 2. Apple Application Model Managers
* **Files:**
  * [apps/apple-reader/AppleReader/ProsodiaModels.swift](file:///Users/lmcfarlin/Projects/Artificial-Humanity/Prosodia/apps/apple-reader/AppleReader/ProsodiaModels.swift)
  * [apps/tuner/ProsodiaTuner/ProsodiaModels.swift](file:///Users/lmcfarlin/Projects/Artificial-Humanity/Prosodia/apps/tuner/ProsodiaTuner/ProsodiaModels.swift)
* **Description:** The Swift model managers discover models on-disk using a hardcoded search priority. These checks look outside the project bundle and bundle roots:
  * Default `modelsBase` config is initialized to `../models` (one directory above the Xcode project root).
  * Discovery candidates search home directory path configurations:
    * `~/Projects/Artificial-Humanity/Prosodia/prosodia_models.json`
    * `~/Projects/Prosodia/prosodia_models.json`
  * Default fallback URL if the config file discovery fails assumes the umbrella workspace layout:
    * `~/Projects/Artificial-Humanity/models`
* **Impact:** Running the built reader apps or Tuner on a machine where the repository is cloned outside `~/Projects/Artificial-Humanity` or `~/Projects/Prosodia` will fail the discovery phase and fall back to bundle resources or environment overrides.

### 3. Model Configuration File
* **File:** [prosodia_models.json](file:///Users/lmcfarlin/Projects/Artificial-Humanity/Prosodia/prosodia_models.json) (Line 2)
* **Description:** The canonical local model configuration declares a relative base pointing outside:
  ```json
  "modelsBase": "../models"
  ```
* **Impact:** This file coordinates where the platforms look for the heavy neural weights relative to their standalone locations.

### 4. Gitignore Configuration
* **File:** [Prosodia/.gitignore](file:///Users/lmcfarlin/Projects/Artificial-Humanity/Prosodia/.gitignore) (Line 77)
* **Description:** Contains documentation about the models layout:
  ```git
  # Models now live at the workspace root (../models), shared across subprojects.
  ```

---

## 🛠️ Recommendations for Resolution

To decouple `Prosodia` from rigid parent-directory requirements:
1. **Test Environment Variables:** Update the Rust actor crate unit tests to look for a `TEST_MODELS_DIR` environment variable. If set, use it to locate model files; otherwise, fall back to the default `../../../models` relative path.
2. **Standardize App Discovery:** Standardize the Swift discovery manager to prioritize relative workspace structures and rely on configurable build flags (or Xcode environment overrides) instead of hardcoding folder names under the user's home directory.
