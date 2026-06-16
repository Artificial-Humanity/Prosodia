//! Voice pack loading and style-vector arithmetic.
//!
//! This is the Rust home for everything the Swift `VoiceLoader` used to do:
//! parsing `.safetensors` voice packs, blending multiple weighted voices, slicing a
//! per-utterance style row, and assembling the 3D style matrix the synthesis engine
//! consumes. Per the architecture plan the platform layer (Swift/Android) is reduced
//! to a single responsibility — handing Rust the raw bytes of a voice file — via the
//! [`VoiceAssetProvider`] callback. All parsing and math live here so every platform
//! shares one implementation.
//!
//! Counting note (matches `chunking`): where the Swift original counted `Character`s
//! (grapheme clusters) we count Unicode scalars (`char`s); for phoneme strings this
//! only differs with combining diacritics.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::asset_manager::StyleVector;
use crate::g2p::TokenPhonemes;
use stage::prosody::CastingProfile;

/// Default cache capacity, mirroring the Swift `LRUCache(limit: 16)`.
const VOICE_CACHE_LIMIT: usize = 16;

/// A single voice and its mixing weight within a blend (the Swift `CastingProfile`).
/// Named `VoiceBlend` here to avoid colliding with the parametric
/// `stage::prosody::CastingProfile` (age/masculinity/strain).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct VoiceBlend {
    pub voice: String,
    pub fraction: f64,
}

/// A named tensor extracted from a `.safetensors` payload.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct NamedStyleVector {
    pub name: String,
    pub vector: StyleVector,
}

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum VoiceLoaderError {
    #[error("safetensors error: {msg}")]
    Safetensors { msg: String },
    #[error("expected a 2D voice pack, got shape {shape:?}")]
    NotTwoDimensional { shape: Vec<u32> },
    #[error("voice shapes in a blend must all match")]
    ShapeMismatch,
    #[error("voice blend was empty")]
    EmptyBlend,
    #[error("missing voice: {voice}")]
    MissingVoice { voice: String },
}

// ---------------------------------------------------------------------------
// Pure functions: parsing and style arithmetic.
// ---------------------------------------------------------------------------

/// Parse a `.safetensors` byte buffer into its named tensors.
///
/// Layout: an 8-byte little-endian header length, a JSON header describing each
/// tensor (`dtype`, `shape`, `data_offsets`), then the tightly packed tensor bytes.
/// Only the dtypes voice packs use are supported (`F32`, `F16`). Structurally
/// malformed entries are skipped (matching the Swift loader); out-of-bounds offsets
/// and unsupported dtypes are hard errors.
#[uniffi::export]
pub fn parse_safetensors(bytes: Vec<u8>) -> Result<Vec<NamedStyleVector>, VoiceLoaderError> {
    if bytes.len() < 8 {
        return Err(VoiceLoaderError::Safetensors {
            msg: "file too small".into(),
        });
    }
    let header_len = u64::from_le_bytes(bytes[0..8].try_into().unwrap()) as usize;
    let header_end = 8 + header_len;
    if bytes.len() < header_end {
        return Err(VoiceLoaderError::Safetensors {
            msg: "invalid header length".into(),
        });
    }

    let header: serde_json::Value =
        serde_json::from_slice(&bytes[8..header_end]).map_err(|e| VoiceLoaderError::Safetensors {
            msg: format!("failed to parse JSON header: {e}"),
        })?;
    let obj = header.as_object().ok_or_else(|| VoiceLoaderError::Safetensors {
        msg: "header is not a JSON object".into(),
    })?;

    let mut result = Vec::new();
    for (key, value) in obj {
        if key == "__metadata__" {
            continue;
        }

        // Skip structurally-incomplete entries, like the Swift `guard ... else { continue }`.
        let (Some(dtype), Some(shape_json), Some(offsets_json)) = (
            value.get("dtype").and_then(|d| d.as_str()),
            value.get("shape").and_then(|s| s.as_array()),
            value.get("data_offsets").and_then(|o| o.as_array()),
        ) else {
            continue;
        };
        if offsets_json.len() != 2 {
            continue;
        }

        let shape: Vec<u32> = shape_json
            .iter()
            .filter_map(|v| v.as_u64().map(|n| n as u32))
            .collect();
        let start = header_end + offsets_json[0].as_u64().unwrap_or(0) as usize;
        let end = header_end + offsets_json[1].as_u64().unwrap_or(0) as usize;
        if bytes.len() < end || end < start {
            return Err(VoiceLoaderError::Safetensors {
                msg: "data offsets out of bounds".into(),
            });
        }
        let tensor = &bytes[start..end];

        let data: Vec<f32> = match dtype {
            "F32" => tensor
                .chunks_exact(4)
                .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect(),
            "F16" => tensor
                .chunks_exact(2)
                .map(|c| f16_bits_to_f32(u16::from_le_bytes([c[0], c[1]])))
                .collect(),
            other => {
                return Err(VoiceLoaderError::Safetensors {
                    msg: format!("unsupported dtype: {other}"),
                })
            }
        };

        result.push(NamedStyleVector {
            name: key.clone(),
            vector: StyleVector { data, shape },
        });
    }

    Ok(result)
}

/// Normalize a freshly loaded voice pack to a 2D `[rows, cols]` shape.
///
/// A genuine 2D pack passes through; a `[d0, 1, d2]` pack has its singleton middle
/// axis squeezed away; anything else is rejected.
#[uniffi::export]
pub fn normalize_style_pack(pack: StyleVector) -> Result<StyleVector, VoiceLoaderError> {
    match pack.shape.len() {
        2 => Ok(pack),
        3 if pack.shape[1] == 1 => Ok(StyleVector {
            data: pack.data,
            shape: vec![pack.shape[0], pack.shape[2]],
        }),
        _ => Err(VoiceLoaderError::NotTwoDimensional { shape: pack.shape }),
    }
}

/// Parse a blend string like `"anchor_female_adult:0.6, anchor_female_child:0.4"`
/// into weighted voices. A bare name (no `:weight`) defaults to weight `1.0`.
#[uniffi::export]
pub fn parse_blend_string(input: String) -> Vec<VoiceBlend> {
    input
        .split(',')
        .filter_map(|part| {
            let mut fields = part.split(':').map(|f| f.trim());
            let name = fields.next().unwrap_or("");
            if name.is_empty() {
                return None;
            }
            let fraction = fields.next().and_then(|w| w.parse::<f64>().ok()).unwrap_or(1.0);
            Some(VoiceBlend {
                voice: name.to_string(),
                fraction,
            })
        })
        .collect()
}

/// Blend already-loaded voice packs by weight, normalizing by the total weight.
///
/// A single pack passes through unchanged (mirroring the Swift single-voice path).
/// All packs must share a shape.
#[uniffi::export]
pub fn blend_style_packs(
    packs: Vec<StyleVector>,
    fractions: Vec<f64>,
) -> Result<StyleVector, VoiceLoaderError> {
    if packs.is_empty() {
        return Err(VoiceLoaderError::EmptyBlend);
    }
    if packs.len() == 1 {
        return Ok(packs.into_iter().next().unwrap());
    }

    let shape = packs[0].shape.clone();
    let mut accumulator = vec![0.0f32; packs[0].data.len()];
    let mut total_fraction = 0.0f64;

    for (pack, fraction) in packs.iter().zip(fractions.iter()) {
        if pack.shape != shape || pack.data.len() != accumulator.len() {
            return Err(VoiceLoaderError::ShapeMismatch);
        }
        let weight = *fraction as f32;
        for (acc, &v) in accumulator.iter_mut().zip(pack.data.iter()) {
            *acc += v * weight;
        }
        total_fraction += *fraction;
    }

    if total_fraction <= 0.0 {
        return Err(VoiceLoaderError::EmptyBlend);
    }
    let denom = total_fraction as f32;
    for acc in accumulator.iter_mut() {
        *acc /= denom;
    }

    Ok(StyleVector {
        data: accumulator,
        shape,
    })
}

/// Slice the single style row for an utterance of `phoneme_count` phonemes.
///
/// The row index is `phoneme_count - 1`, clamped into the pack's row range, yielding
/// a `[1, cols]` vector.
#[uniffi::export]
pub fn slice_style_row(
    pack: StyleVector,
    phoneme_count: i64,
) -> Result<StyleVector, VoiceLoaderError> {
    if pack.shape.len() != 2 {
        return Err(VoiceLoaderError::NotTwoDimensional { shape: pack.shape });
    }
    let rows = pack.shape[0] as i64;
    let cols = pack.shape[1] as usize;
    let index = (phoneme_count - 1).clamp(0, (rows - 1).max(0)) as usize;
    let start = index * cols;
    let row = pack.data[start..start + cols].to_vec();
    Ok(StyleVector {
        data: row,
        shape: vec![1, cols as u32],
    })
}

/// Convert IEEE-754 half-precision bits to `f32`.
fn f16_bits_to_f32(h: u16) -> f32 {
    let sign = if (h >> 15) & 1 == 1 { -1.0f32 } else { 1.0f32 };
    let exponent = (h >> 10) & 0x1f;
    let fraction = (h & 0x3ff) as f32;
    match exponent {
        0 => sign * fraction * 2f32.powi(-24), // subnormal (and zero)
        0x1f => {
            if fraction == 0.0 {
                sign * f32::INFINITY
            } else {
                f32::NAN
            }
        }
        _ => sign * (1.0 + fraction / 1024.0) * 2f32.powi(exponent as i32 - 15),
    }
}

// ---------------------------------------------------------------------------
// VoiceLoader: stateful orchestration over a byte provider.
// ---------------------------------------------------------------------------

/// Platform hook: return the raw bytes of a voice file by name (no parsing).
/// Swift/Android implement this as a simple file read.
#[uniffi::export(callback_interface)]
pub trait VoiceAssetProvider: Send + Sync {
    fn load_voice_bytes(&self, voice_name: String) -> Option<Vec<u8>>;
}

/// Loads, caches, blends, and slices voice style packs entirely in Rust, sourcing
/// raw bytes from a [`VoiceAssetProvider`].
#[derive(uniffi::Object)]
pub struct VoiceLoader {
    provider: Box<dyn VoiceAssetProvider>,
    /// Parsed, normalized packs keyed by voice name, with LRU eviction at the limit.
    cache: Mutex<VoiceCache>,
}

#[derive(Default)]
struct VoiceCache {
    packs: HashMap<String, StyleVector>,
    order: Vec<String>,
}

impl VoiceCache {
    fn get(&mut self, name: &str) -> Option<StyleVector> {
        if let Some(vector) = self.packs.get(name).cloned() {
            // Move the accessed name to the end of the LRU tracking queue
            if let Some(pos) = self.order.iter().position(|x| x == name) {
                self.order.remove(pos);
            }
            self.order.push(name.to_string());
            Some(vector)
        } else {
            None
        }
    }

    fn insert(&mut self, name: String, pack: StyleVector) {
        if self.packs.insert(name.clone(), pack).is_some() {
            // Update accessed order if it already exists
            if let Some(pos) = self.order.iter().position(|x| x == &name) {
                self.order.remove(pos);
            }
            self.order.push(name);
        } else {
            // New entry: push to end of order
            self.order.push(name);
            while self.order.len() > VOICE_CACHE_LIMIT {
                let oldest = self.order.remove(0);
                self.packs.remove(&oldest);
            }
        }
    }
}

#[uniffi::export]
impl VoiceLoader {
    #[uniffi::constructor]
    pub fn new(provider: Box<dyn VoiceAssetProvider>) -> Arc<Self> {
        Arc::new(Self {
            provider,
            cache: Mutex::new(VoiceCache::default()),
        })
    }

    /// Clear the in-memory voice cache.
    pub fn clear_cache(&self) {
        let mut cache = self.cache.lock().unwrap();
        cache.packs.clear();
        cache.order.clear();
    }

    /// Linearly interpolates two style vectors.
    pub fn lerp(&self, v1: &StyleVector, v2: &StyleVector, fraction: f32) -> StyleVector {
        let f = fraction.clamp(0.0, 1.0);
        let mut interpolated = vec![0.0; v1.data.len()];
        for i in 0..v1.data.len() {
            let val1 = v1.data.get(i).copied().unwrap_or(0.0);
            let val2 = v2.data.get(i).copied().unwrap_or(0.0);
            interpolated[i] = val1 * (1.0 - f) + val2 * f;
        }
        StyleVector {
            data: interpolated,
            shape: v1.shape.clone(),
        }
    }

    /// Performs bilinear interpolation to resolve a custom speaker identity vector.
    pub fn resolve_parametric_voice(&self, casting: &CastingProfile) -> Result<StyleVector, VoiceLoaderError> {
        let v_fc = self.load_voice("anchor_female_child".to_string())?;
        let v_mc = self.load_voice("anchor_male_child".to_string())?;
        let v_fa = self.load_voice("anchor_female_adult".to_string())?;
        let v_ma = self.load_voice("anchor_male_adult".to_string())?;
        let v_fe = self.load_voice("anchor_female_elderly".to_string())?;
        let v_me = self.load_voice("anchor_male_elderly".to_string())?;
        
        let age = casting.age_profile as f32;
        let masc = casting.masculinity as f32;
        
        let identity = if age <= 0.5 {
            let scale_age = age * 2.0;
            let female_interp = self.lerp(&v_fc, &v_fa, scale_age);
            let male_interp = self.lerp(&v_mc, &v_ma, scale_age);
            self.lerp(&female_interp, &male_interp, masc)
        } else {
            let scale_age = (age - 0.5) * 2.0;
            let female_interp = self.lerp(&v_fa, &v_fe, scale_age);
            let male_interp = self.lerp(&v_ma, &v_me, scale_age);
            self.lerp(&female_interp, &male_interp, masc)
        };
        
        // Blend in texture anchors based on raspiness
        if casting.strain_or_rasp > 0.05 {
            let s_gruff = self.load_voice("anchor_style_gruff".to_string())?;
            return Ok(self.lerp(&identity, &s_gruff, casting.strain_or_rasp as f32));
        }
        
        Ok(identity)
    }

    /// Load and normalize a single voice pack by name (cached).
    pub fn load_voice(&self, voice_name: String) -> Result<StyleVector, VoiceLoaderError> {
        if let Some(cached) = self.cache.lock().unwrap().get(&voice_name) {
            return Ok(cached);
        }

        let bytes = self
            .provider
            .load_voice_bytes(voice_name.clone())
            .ok_or_else(|| VoiceLoaderError::MissingVoice {
                voice: voice_name.clone(),
            })?;

        let tensors = parse_safetensors(bytes)?;
        let first = tensors
            .into_iter()
            .next()
            .ok_or_else(|| VoiceLoaderError::MissingVoice {
                voice: voice_name.clone(),
            })?;
        let pack = normalize_style_pack(first.vector)?;

        self.cache_insert(voice_name, pack.clone());
        Ok(pack)
    }

    /// Load a weighted blend of voices, normalized by total weight.
    pub fn load_blend(&self, blend: Vec<VoiceBlend>) -> Result<StyleVector, VoiceLoaderError> {
        if blend.is_empty() {
            return Err(VoiceLoaderError::EmptyBlend);
        }
        if blend.len() == 1 {
            return self.load_voice(blend[0].voice.clone());
        }
        let mut packs = Vec::with_capacity(blend.len());
        let mut fractions = Vec::with_capacity(blend.len());
        for target in &blend {
            packs.push(self.load_voice(target.voice.clone())?);
            fractions.push(target.fraction);
        }
        blend_style_packs(packs, fractions)
    }

    /// Resolve the `[1, cols]` style row for a voice (or comma-separated blend string)
    /// at the given phoneme count.
    pub fn style_vector(
        &self,
        voice: String,
        phoneme_count: i64,
    ) -> Result<StyleVector, VoiceLoaderError> {
        let blend = parse_blend_string(voice);
        let pack = self.load_blend(blend)?;
        slice_style_row(pack, phoneme_count)
    }

    /// Build the 3D `[1, total_rows, cols]` style matrix mapping each token to its
    /// resolved voice-blend embedding, framed by SOS/EOS rows.
    pub fn style_matrix(
        &self,
        tokens: Vec<TokenPhonemes>,
        voice_blends: Vec<String>,
        vocab: HashMap<String, i32>,
    ) -> Result<StyleVector, VoiceLoaderError> {
        if tokens.is_empty() || voice_blends.is_empty() {
            return Err(VoiceLoaderError::EmptyBlend);
        }

        // Per-token "phonemes + whitespace", with the same leading/trailing whitespace
        // trim the synthesis pipeline applies to the concatenated chunk.
        let mut token_chars: Vec<Vec<char>> = tokens
            .iter()
            .map(|t| format!("{}{}", t.phonemes, t.whitespace).chars().collect())
            .collect();
        trim_leading_whitespace(&mut token_chars);
        trim_trailing_whitespace(&mut token_chars);

        let valid_counts: Vec<usize> = token_chars
            .iter()
            .map(|chars| count_in_vocab(chars, &vocab))
            .collect();
        let total_phonemes: i64 = 2 + valid_counts.iter().sum::<usize>() as i64;

        let style_for = |blend_str: &str| -> Result<StyleVector, VoiceLoaderError> {
            let blend = parse_blend_string(blend_str.to_string());
            let pack = self.load_blend(blend)?;
            slice_style_row(pack, total_phonemes)
        };

        let mut style_arrays: Vec<StyleVector> = Vec::new();

        // SOS row.
        style_arrays.push(style_for(&voice_blends[0])?);

        // One block per token, each row repeated once per in-vocab phoneme.
        let mut last_row_data: Option<Vec<f32>> = None;
        for (idx, valid) in valid_counts.iter().copied().enumerate() {
            if valid == 0 {
                continue;
            }
            let blend_index = idx.min(voice_blends.len() - 1);
            let mut row = style_for(&voice_blends[blend_index])?;
            if let Some(ref prev) = last_row_data {
                for (r, p) in row.data.iter_mut().zip(prev.iter()) {
                    *r = 0.5 * *r + 0.5 * p;
                }
            }
            last_row_data = Some(row.data.clone());

            let cols = row.shape[1] as usize;
            let mut repeated = Vec::with_capacity(valid * row.data.len());
            for _ in 0..valid {
                repeated.extend_from_slice(&row.data);
            }
            style_arrays.push(StyleVector {
                data: repeated,
                shape: vec![valid as u32, cols as u32],
            });
        }

        // EOS row.
        style_arrays.push(style_for(voice_blends.last().unwrap())?);

        let cols = style_arrays
            .first()
            .and_then(|s| s.shape.get(1).copied())
            .unwrap_or(0);
        let mut data = Vec::new();
        let mut total_rows = 0u32;
        for style in &style_arrays {
            data.extend_from_slice(&style.data);
            total_rows += style.shape[0];
        }

        Ok(StyleVector {
            data,
            shape: vec![1, total_rows, cols],
        })
    }
}

impl VoiceLoader {
    fn cache_insert(&self, name: String, pack: StyleVector) {
        self.cache.lock().unwrap().insert(name, pack);
    }
}

/// Count how many of `chars` are single-character keys present in `vocab`.
fn count_in_vocab(chars: &[char], vocab: &HashMap<String, i32>) -> usize {
    chars
        .iter()
        .filter(|c| vocab.contains_key(&c.to_string()))
        .count()
}

/// Drop leading whitespace across the token boundary, matching the pipeline's
/// concatenated-chunk trim.
fn trim_leading_whitespace(tokens: &mut [Vec<char>]) {
    let mut remaining = tokens
        .iter()
        .flatten()
        .take_while(|c| c.is_whitespace())
        .count();
    for token in tokens.iter_mut() {
        if remaining == 0 {
            break;
        }
        if remaining >= token.len() {
            remaining -= token.len();
            token.clear();
        } else {
            token.drain(0..remaining);
            remaining = 0;
        }
    }
}

/// Drop trailing whitespace across the token boundary.
fn trim_trailing_whitespace(tokens: &mut [Vec<char>]) {
    let mut remaining = tokens
        .iter()
        .rev()
        .flat_map(|t| t.iter().rev())
        .take_while(|c| c.is_whitespace())
        .count();
    for token in tokens.iter_mut().rev() {
        if remaining == 0 {
            break;
        }
        if remaining >= token.len() {
            remaining -= token.len();
            token.clear();
        } else {
            let keep = token.len() - remaining;
            token.truncate(keep);
            remaining = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a single-tensor F32 `.safetensors` blob with the given shape and data.
    fn build_safetensors_f32(name: &str, shape: &[u32], data: &[f32]) -> Vec<u8> {
        let mut payload = Vec::new();
        for &f in data {
            payload.extend_from_slice(&f.to_le_bytes());
        }
        let shape_json: Vec<String> = shape.iter().map(|d| d.to_string()).collect();
        let header = format!(
            r#"{{"{name}":{{"dtype":"F32","shape":[{}],"data_offsets":[0,{}]}}}}"#,
            shape_json.join(","),
            payload.len()
        );
        let header_bytes = header.into_bytes();

        let mut out = Vec::new();
        out.extend_from_slice(&(header_bytes.len() as u64).to_le_bytes());
        out.extend_from_slice(&header_bytes);
        out.extend_from_slice(&payload);
        out
    }

    fn vocab_from(chars: &[&str]) -> HashMap<String, i32> {
        chars
            .iter()
            .enumerate()
            .map(|(i, c)| (c.to_string(), i as i32))
            .collect()
    }

    /// In-memory provider backed by a name -> bytes map.
    struct MapProvider {
        map: HashMap<String, Vec<u8>>,
    }
    impl VoiceAssetProvider for MapProvider {
        fn load_voice_bytes(&self, voice_name: String) -> Option<Vec<u8>> {
            self.map.get(&voice_name).cloned()
        }
    }

    #[test]
    fn parses_f32_safetensors() {
        let blob = build_safetensors_f32("style", &[2, 3], &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        let tensors = parse_safetensors(blob).unwrap();
        assert_eq!(tensors.len(), 1);
        assert_eq!(tensors[0].name, "style");
        assert_eq!(tensors[0].vector.shape, vec![2, 3]);
        assert_eq!(tensors[0].vector.data, vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
    }

    #[test]
    fn parse_blend_string_parses_weights_and_defaults() {
        assert_eq!(
            parse_blend_string("a:0.6, b:0.4".to_string()),
            vec![
                VoiceBlend { voice: "a".into(), fraction: 0.6 },
                VoiceBlend { voice: "b".into(), fraction: 0.4 },
            ]
        );
        assert_eq!(
            parse_blend_string("solo".to_string()),
            vec![VoiceBlend { voice: "solo".into(), fraction: 1.0 }]
        );
    }

    #[test]
    fn normalize_squeezes_singleton_middle_axis() {
        let pack = StyleVector { data: vec![1.0, 2.0, 3.0, 4.0], shape: vec![2, 1, 2] };
        assert_eq!(normalize_style_pack(pack).unwrap().shape, vec![2, 2]);
    }

    #[test]
    fn blend_is_weighted_average() {
        let a = StyleVector { data: vec![0.0, 0.0], shape: vec![1, 2] };
        let b = StyleVector { data: vec![10.0, 20.0], shape: vec![1, 2] };
        // weights 0.25 / 0.75 -> (0*0.25 + 10*0.75)/1.0 = 7.5, etc.
        let blended = blend_style_packs(vec![a, b], vec![0.25, 0.75]).unwrap();
        assert_eq!(blended.data, vec![7.5, 15.0]);
    }

    #[test]
    fn blend_mismatched_shapes_errors() {
        let a = StyleVector { data: vec![0.0, 0.0], shape: vec![1, 2] };
        let b = StyleVector { data: vec![1.0], shape: vec![1, 1] };
        assert!(matches!(
            blend_style_packs(vec![a, b], vec![0.5, 0.5]),
            Err(VoiceLoaderError::ShapeMismatch)
        ));
    }

    #[test]
    fn slice_row_clamps_index() {
        let pack = StyleVector {
            data: vec![1.0, 2.0, /**/ 3.0, 4.0, /**/ 5.0, 6.0],
            shape: vec![3, 2],
        };
        // phoneme_count 2 -> index 1 -> row [3,4]
        assert_eq!(slice_style_row(pack.clone(), 2).unwrap().data, vec![3.0, 4.0]);
        // phoneme_count 99 -> clamp to last row [5,6]
        assert_eq!(slice_style_row(pack.clone(), 99).unwrap().data, vec![5.0, 6.0]);
        // phoneme_count 0 -> clamp to first row [1,2]
        assert_eq!(slice_style_row(pack, 0).unwrap().data, vec![1.0, 2.0]);
    }

    #[test]
    fn voice_loader_loads_caches_and_blends() {
        // Two voices, each a 2-row x 2-col pack so slicing has something to pick.
        let va = build_safetensors_f32("s", &[2, 2], &[0.0, 0.0, 0.0, 0.0]);
        let vb = build_safetensors_f32("s", &[2, 2], &[10.0, 10.0, 10.0, 10.0]);
        let mut map = HashMap::new();
        map.insert("a".to_string(), va);
        map.insert("b".to_string(), vb);
        let loader = VoiceLoader::new(Box::new(MapProvider { map }));

        // Single voice load + cache hit.
        let pack = loader.load_voice("a".to_string()).unwrap();
        assert_eq!(pack.shape, vec![2, 2]);
        let again = loader.load_voice("a".to_string()).unwrap();
        assert_eq!(pack, again);

        // Blend average of a (0) and b (10) at 50/50 -> 5.
        let blended = loader.load_blend(vec![
            VoiceBlend { voice: "a".into(), fraction: 0.5 },
            VoiceBlend { voice: "b".into(), fraction: 0.5 },
        ]).unwrap();
        assert_eq!(blended.data, vec![5.0, 5.0, 5.0, 5.0]);

        // Missing voice surfaces an error.
        assert!(matches!(
            loader.load_voice("ghost".to_string()),
            Err(VoiceLoaderError::MissingVoice { .. })
        ));
    }

    #[test]
    fn style_matrix_frames_tokens_with_sos_eos() {
        // One voice: 4 rows x 2 cols, each row = [row_index, row_index].
        let data: Vec<f32> = (0..4).flat_map(|r| [r as f32, r as f32]).collect();
        let blob = build_safetensors_f32("s", &[4, 2], &data);
        let mut map = HashMap::new();
        map.insert("v".to_string(), blob);
        let loader = VoiceLoader::new(Box::new(MapProvider { map }));

        // Two tokens "ab" and "c" (no whitespace), vocab covers a,b,c.
        let tokens = vec![
            TokenPhonemes { phonemes: "ab".into(), whitespace: "".into() },
            TokenPhonemes { phonemes: "c".into(), whitespace: "".into() },
        ];
        let vocab = vocab_from(&["a", "b", "c"]);
        let matrix = loader
            .style_matrix(tokens, vec!["v".to_string()], vocab)
            .unwrap();

        // total_phonemes = 2 + (2 + 1) = 5 -> sliced row index = min(4, rows-1)=3.
        // Rows: SOS(1) + token0(2 phonemes) + token1(1 phoneme) + EOS(1) = 5 rows, 2 cols.
        assert_eq!(matrix.shape, vec![1, 5, 2]);
        assert_eq!(matrix.data.len(), 5 * 2);
        // Every row is row index 3 -> [3,3].
        assert!(matrix.data.iter().all(|&v| v == 3.0));
    }

    #[test]
    fn style_matrix_ema_smoothing() {
        // Voice A: row 0 is [1.0, 1.0]
        let va = build_safetensors_f32("s", &[1, 2], &[1.0, 1.0]);
        // Voice B: row 0 is [3.0, 3.0]
        let vb = build_safetensors_f32("s", &[1, 2], &[3.0, 3.0]);
        let mut map = HashMap::new();
        map.insert("a".to_string(), va);
        map.insert("b".to_string(), vb);
        let loader = VoiceLoader::new(Box::new(MapProvider { map }));

        // Two tokens "x" and "y"
        let tokens = vec![
            TokenPhonemes { phonemes: "x".into(), whitespace: "".into() },
            TokenPhonemes { phonemes: "y".into(), whitespace: "".into() },
        ];
        let vocab = vocab_from(&["x", "y"]);
        // Token 0 blend: "a", Token 1 blend: "b"
        let matrix = loader
            .style_matrix(tokens, vec!["a".to_string(), "b".to_string()], vocab)
            .unwrap();

        // total_phonemes = 2 + 2 = 4.
        // Rows: SOS(1) + token 0(1) + token 1(1) + EOS(1) = 4 rows, 2 cols.
        assert_eq!(matrix.shape, vec![1, 4, 2]);
        
        // Token 0: style for "a" -> [1.0, 1.0]. Unsmoothed because it's first.
        // Token 1: style for "b" -> [3.0, 3.0]. Smoothed with Token 0: 0.5 * [3.0, 3.0] + 0.5 * [1.0, 1.0] = [2.0, 2.0].
        let row_token0 = &matrix.data[2..4]; // SOS is index 0..2
        let row_token1 = &matrix.data[4..6];
        
        assert_eq!(row_token0, &[1.0, 1.0]);
        assert_eq!(row_token1, &[2.0, 2.0]);
    }

    #[test]
    fn test_resolve_parametric_voice() {
        let mut map = HashMap::new();
        map.insert("anchor_female_child".into(), build_safetensors_f32("s", &[1, 2], &[10.0, 10.0]));
        map.insert("anchor_male_child".into(), build_safetensors_f32("s", &[1, 2], &[20.0, 20.0]));
        map.insert("anchor_female_adult".into(), build_safetensors_f32("s", &[1, 2], &[30.0, 30.0]));
        map.insert("anchor_male_adult".into(), build_safetensors_f32("s", &[1, 2], &[40.0, 40.0]));
        map.insert("anchor_female_elderly".into(), build_safetensors_f32("s", &[1, 2], &[50.0, 50.0]));
        map.insert("anchor_male_elderly".into(), build_safetensors_f32("s", &[1, 2], &[60.0, 60.0]));
        map.insert("anchor_style_gruff".into(), build_safetensors_f32("s", &[1, 2], &[100.0, 100.0]));

        let loader = VoiceLoader::new(Box::new(MapProvider { map }));

        let casting = CastingProfile {
            age_profile: 0.25,
            masculinity: 0.5,
            strain_or_rasp: 0.0,
        };
        let res = loader.resolve_parametric_voice(&casting).unwrap();
        assert_eq!(res.data, vec![25.0, 25.0]);

        let casting2 = CastingProfile {
            age_profile: 0.75,
            masculinity: 0.5,
            strain_or_rasp: 0.0,
        };
        let res2 = loader.resolve_parametric_voice(&casting2).unwrap();
        assert_eq!(res2.data, vec![45.0, 45.0]);

        let casting3 = CastingProfile {
            age_profile: 0.25,
            masculinity: 0.5,
            strain_or_rasp: 0.2,
        };
        let res3 = loader.resolve_parametric_voice(&casting3).unwrap();
        assert_eq!(res3.data, vec![40.0, 40.0]);
    }

    #[test]
    fn test_lru_cache_eviction() {
        let mut map = HashMap::new();
        // Create 18 distinct voices
        for i in 0..18 {
            let va = build_safetensors_f32("s", &[1, 2], &[i as f32, i as f32]);
            map.insert(i.to_string(), va);
        }
        let loader = VoiceLoader::new(Box::new(MapProvider { map }));

        // 1. Fill the cache up to capacity (VOICE_CACHE_LIMIT = 16) with voices "0" to "15"
        for i in 0..16 {
            loader.load_voice(i.to_string()).unwrap();
        }

        // Cache order: "0", "1", "2", ..., "15"
        {
            let cache = loader.cache.lock().unwrap();
            assert_eq!(cache.order.len(), 16);
            assert_eq!(cache.order[0], "0");
            assert_eq!(cache.order[15], "15");
        }

        // 2. Access "0" again to make it the most recently used (should move to end of order)
        loader.load_voice("0".to_string()).unwrap();
        {
            let cache = loader.cache.lock().unwrap();
            assert_eq!(cache.order.len(), 16);
            assert_eq!(cache.order[15], "0");
            assert_eq!(cache.order[0], "1"); // "1" is now the oldest (least recently used)
        }

        // 3. Load a new voice "16" which triggers eviction.
        // The oldest voice "1" should be evicted.
        loader.load_voice("16".to_string()).unwrap();
        {
            let cache = loader.cache.lock().unwrap();
            assert_eq!(cache.order.len(), 16);
            assert!(!cache.packs.contains_key("1")); // voice "1" evicted!
            assert!(cache.packs.contains_key("0"));  // voice "0" still present!
            assert_eq!(cache.order[15], "16");
        }
    }
}
