"""Reference renders for the split runtime's parity tests.

An independent NumPy implementation of Sonora's host pipeline for the LiteRT
split graphs (`host_pipeline_vat` in Sonora's
`scripts/litert_export/convert_vat.py`; the unconditioned recipe is the same
pipeline without `spk`/`vat`). It runs the same pinned graphs as
`crates/actor/src/split_engine.rs` and writes, per case, the reference audio
the Rust tests must reproduce.

The ODE's initial state is not stored: both sides draw it from the same seeded
xorshift64* + Box–Muller stream (`GaussianRng` in split_engine.rs), so a
fixture is the case parameters plus the reference waveform.

Fixtures are tied to the model pins in `prosodia_models.json`: regenerate them
whenever a pinned split role changes, after checking with Sonora's resident.

Run on a machine with the Sonora registry checkout beside Prosodia, with NumPy
and `ai-edge-litert`, for example on ai-lab-0:

    /data/toolchain/litert-conversion/.venv/bin/python \\
        crates/actor/tests/fixtures/split_parity/generate.py
"""

import json
import math
import os
from pathlib import Path

import numpy as np
from ai_edge_litert.interpreter import Interpreter

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[4]
MODELS_CONFIG = REPO / "prosodia_models.json"

PHRASE = "ðə kwˈɪk bɹˈaʊn fˈɑːks dʒˈʌmps ˌoʊvɚ ðə lˈeɪzi dˈɑːɡ."
TEMPERATURE = 0.667
SEED = 0x5EED_1234
N_TIMESTEPS = 10

# (role in prosodia_models.json, fixture name, cases as (speaker, vat)).
# derisk-energy-24k trained only vat[1] (energy); the cases stay on it.
MODELS = [
    ("actor-split", "baseline-ljspeech-22k", [(0, None)]),
    ("actor-split-24k", "derisk-energy-24k", [(100, [0.0, 0.7, 0.0]), (17, [0.0, -1.0, 0.0])]),
]

MASK64 = (1 << 64) - 1


class GaussianRng:
    """Bit-for-bit port of split_engine.rs `GaussianRng` (f32 Box–Muller)."""

    def __init__(self, seed):
        self.state = (seed | 1) & MASK64
        self.spare = None

    def next_u64(self):
        x = self.state
        x ^= x >> 12
        x ^= (x << 25) & MASK64
        x ^= x >> 27
        self.state = x
        return (x * 0x2545F4914F6CDD1D) & MASK64

    def next_uniform(self):
        return np.float32(((self.next_u64() >> 40) + 1) / float(1 << 24))

    def next_gaussian(self):
        if self.spare is not None:
            s, self.spare = self.spare, None
            return s
        u1 = self.next_uniform()
        u2 = self.next_uniform()
        r = np.sqrt(np.float32(-2.0) * np.log(u1))
        theta = np.float32(2.0 * math.pi) * u2
        self.spare = r * np.sin(theta)
        return r * np.cos(theta)


def initial_state(n_feats, max_mel):
    rng = GaussianRng(SEED)
    z = np.array([rng.next_gaussian() for _ in range(n_feats * max_mel)], np.float32)
    return (z * np.float32(TEMPERATURE)).reshape(1, n_feats, max_mel)


def sin_pos_emb(t, dim, scale=1000.0):
    half = dim // 2
    k = -math.log(10000.0) / (half - 1)
    freqs = np.exp(np.arange(half, dtype=np.float32) * np.float32(k)).astype(np.float32)
    ang = (np.float32(scale) * np.float32(t) * freqs).astype(np.float32)
    return np.concatenate([np.sin(ang), np.cos(ang)])[None].astype(np.float32)


def run(interpreter, args):
    for detail, value in zip(interpreter.get_input_details(), args):
        interpreter.set_tensor(detail["index"], value.astype(np.float32).reshape(detail["shape"]))
    interpreter.invoke()
    return [interpreter.get_tensor(o["index"]) for o in interpreter.get_output_details()]


def graph(model_dir, role):
    for family in ("matcha", "sonora"):
        found = sorted(model_dir.glob(f"{family}_{role}*.tflite"))
        if found:
            interpreter = Interpreter(model_path=str(found[0]))
            interpreter.allocate_tensors()
            return interpreter
    raise FileNotFoundError(f"no {role} graph in {model_dir}")


def render(model_dir, cfg, ids, speaker, vat):
    max_text, max_mel = cfg["MAX_TEXT"], cfg["MAX_MEL"]
    textenc, decoder, vocoder = (graph(model_dir, r) for r in ("textenc", "decoder", "vocoder"))
    emb = np.fromfile(model_dir / "emb.bin", "<f4").reshape(cfg["n_vocab"], cfg["n_channels"])
    t_emb_dim = int(decoder.get_input_details()[2]["shape"][1])
    conditioned = len(textenc.get_input_details()) == 4

    t_x = len(ids)
    ids_pad = np.zeros(max_text, int)
    ids_pad[:t_x] = ids
    tmask = np.zeros((1, 1, max_text), np.float32)
    tmask[0, 0, :t_x] = 1.0
    args = [emb[ids_pad][None], tmask]
    if conditioned:
        spk_dim = int(textenc.get_input_details()[2]["shape"][1])
        vat_dim = int(textenc.get_input_details()[3]["shape"][1])
        spk_vec = np.fromfile(model_dir / "spk_emb.bin", "<f4").reshape(-1, spk_dim)[speaker][None]
        values = np.array(vat if vat is not None else [0.0] * vat_dim, np.float32)
        vat_tok = np.repeat(values.reshape(1, vat_dim, 1), max_text, axis=2) * tmask
        args += [spk_vec, vat_tok]
    outs = run(textenc, args)
    mu_x = next(o for o in outs if o.shape[1] == cfg["n_feats"])
    logw = next(o for o in outs if o.shape[1] == 1)

    w_ceil = np.ceil(np.exp(logw) * tmask) * cfg["length_scale"]
    y_len = max(int(w_ceil.sum()), 1)
    ymask = np.zeros((1, 1, max_mel), np.float32)
    ymask[0, 0, :y_len] = 1.0
    # generate_path: frame f belongs to token t when cum[t-1] <= f < cum[t].
    cum = np.cumsum(w_ceil[0, 0, :])
    path = np.zeros((max_text, max_mel), np.float32)
    prev = np.zeros(max_mel, np.float32)
    for t in range(max_text):
        cur = (np.arange(max_mel) < cum[t]).astype(np.float32)
        path[t] = cur - prev
        prev = cur
    path *= tmask[0, 0, :, None] * ymask[0, 0, None, :]
    mu_y = (mu_x[0] @ path)[None]

    x = initial_state(cfg["n_feats"], max_mel) * ymask
    ts = np.linspace(0, 1, N_TIMESTEPS + 1, dtype=np.float32)
    t, dt = ts[0], ts[1] - ts[0]
    for step in range(1, N_TIMESTEPS + 1):
        dargs = [x, mu_y, sin_pos_emb(t, t_emb_dim), ymask]
        if conditioned:
            dargs += [spk_vec, (vat_tok[0] @ path)[None]]
        x = x + dt * run(decoder, dargs)[0]
        t = t + dt
        if step < N_TIMESTEPS:
            dt = ts[step + 1] - t
    mel = (x * cfg["mel_std"] + cfg["mel_mean"]) * ymask
    wav = np.clip(run(vocoder, [mel])[0], -1, 1).reshape(-1)[: y_len * cfg["hop"]]
    return wav, y_len


def main():
    models_cfg = json.loads(MODELS_CONFIG.read_text())
    revision = models_cfg["registry"]["revision"]
    for role, name, cases in MODELS:
        # Lexical, as ProsodiaModelsManager resolves it: `..` must not follow a
        # symlinked models/ directory.
        model_dir = Path(os.path.normpath(
            MODELS_CONFIG.parent / models_cfg["modelsBase"] / models_cfg["roles"][role]["path"]
        ))
        cfg = json.loads((model_dir / "config.json").read_text())
        symbols = cfg["symbols"]
        ids = [0]
        for ch in PHRASE:
            ids += [symbols.index(ch), 0]
        out_dir = HERE / name
        out_dir.mkdir(exist_ok=True)
        meta = {
            "role": role,
            "registry_revision": revision,
            "phrase": PHRASE,
            "ids": ids,
            "temperature": TEMPERATURE,
            "seed": SEED,
            "cases": [],
        }
        for i, (speaker, vat) in enumerate(cases):
            wav, y_len = render(model_dir, cfg, ids, speaker, vat)
            pcm = np.round(wav * 32767.0).astype("<i2")
            pcm.tofile(out_dir / f"case{i}.pcm16")
            meta["cases"].append({"speaker": speaker, "vat": vat, "y_lengths": y_len, "pcm": f"case{i}.pcm16"})
            print(f"{name} case {i}: speaker {speaker} vat {vat} -> {y_len} frames, {len(pcm)} samples")
        (out_dir / "meta.json").write_text(json.dumps(meta, indent=2, ensure_ascii=False) + "\n")


if __name__ == "__main__":
    main()
