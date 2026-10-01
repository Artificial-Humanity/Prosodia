"""Reference phoneme strings for Prosodia's G2P parity measurement.

Sonora's export gate G7 requires the device text front end to reproduce the
TRAINING front end's phoneme string exactly. The oracle is the training front
end itself, `matcha.text.op_g2p.OpenPhonemizerG2P` with homographs off, over
the probe corpus G7 uses: `device_g2p.probe_sentences(op_g2p.contraction_tables())`
(PARITY_PROBES plus one sentence per contraction-table entry).

This writes `reference.json` (the probes, their reference strings, the locked
symbol table and the Sonora commit they came from) for the Rust measurement in
`crates/actor/src/g2p_parity.rs`. Regenerate when Sonora's front end, probes or
symbols change; check with Sonora's resident first.

Runs read-only inside the Sonora checkout's environment, for example on
ai-lab-0 from the Prosodia repo root:

    ../Sonora/github/.venv/bin/python crates/actor/tests/fixtures/g2p_parity/generate.py
"""

import hashlib
import json
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
SONORA = HERE.parents[5] / "Sonora" / "github"

sys.path.insert(0, str(SONORA))
sys.path.insert(0, str(SONORA / "scripts" / "litert_export"))

import device_g2p  # noqa: E402
from matcha.text import op_g2p  # noqa: E402
from matcha.text.symbols import symbols  # noqa: E402


def sha256_file(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    commit = subprocess.run(
        ["git", "-C", str(SONORA), "rev-parse", "HEAD"], capture_output=True, text=True, check=True
    ).stdout.strip()
    dirty = subprocess.run(
        ["git", "-C", str(SONORA), "status", "--porcelain", "--", "matcha/text", "scripts/litert_export"],
        capture_output=True, text=True, check=True,
    ).stdout.strip()
    if dirty:
        sys.exit(f"Sonora's front end has uncommitted changes; the fixture would not be reproducible:\n{dirty}")

    host = op_g2p.OpenPhonemizerG2P(use_neural_oov=True, homographs=False)
    assets = Path(host.assets_dir)
    # Count every call into the neural OOV graph. `stats["neural_hits"]` misses the
    # calls made while resolving apostrophe words (possessives, clitics, the
    # bare-letters fallback for names such as O'Brien), which go through
    # `_plain_word` -> `_neural_word` without incrementing it.
    neural_calls = [0]
    neural_word = host._neural_word

    def counted_neural_word(word):
        neural_calls[0] += 1
        return neural_word(word)

    host._neural_word = counted_neural_word
    raw_tables = op_g2p.contraction_tables()
    # G7 writes this exact string as g2p_contractions.json; its hash identifies the tables.
    tables_json = json.dumps(raw_tables, ensure_ascii=False, indent=2, sort_keys=True)
    tables = json.loads(json.dumps(raw_tables, ensure_ascii=False))

    fixed = set(device_g2p.PARITY_PROBES)
    probes = []
    for text in device_g2p.probe_sentences(tables):
        neural_before = neural_calls[0]
        ipa = host.phonemize(text)
        probes.append({
            "text": text,
            "ipa": ipa,
            # The table-driven sentences, one per contraction entry. Sonora's pinned
            # models trained before the contraction table existed (2026-08-02), so a
            # mismatch here is distance from the spec, not necessarily audible.
            "contraction": text not in fixed,
            # The host reached its neural OOV graph: a mismatch here compares two OOV
            # models, not two lexicons.
            "neural": neural_calls[0] > neural_before,
        })

    out = {
        "oracle": "matcha.text.op_g2p.OpenPhonemizerG2P(use_neural_oov=True, homographs=False).phonemize",
        "probes_from": "device_g2p.probe_sentences(op_g2p.contraction_tables())",
        "sonora_commit": commit,
        "homographs": False,
        "assets_dir": str(assets),
        "sha256": {
            "g2p_dict.txt.gz": sha256_file(assets / "g2p_dict.txt.gz"),
            "g2p_meta.json": sha256_file(assets / "g2p_meta.json"),
            "dp_g2p_matcha_fp16.tflite": sha256_file(assets / "dp_g2p_matcha_fp16.tflite"),
            "g2p_contractions.json": hashlib.sha256(tables_json.encode("utf-8")).hexdigest(),
        },
        "symbols": list(symbols),
        "probes": probes,
    }
    (HERE / "reference.json").write_text(json.dumps(out, indent=2, ensure_ascii=False) + "\n")
    print(f"{len(probes)} probes from Sonora {commit[:9]}")


if __name__ == "__main__":
    main()
