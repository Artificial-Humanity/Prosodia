"""Oracle fixtures for Prosodia's port of Sonora's device G2P.

Runs Sonora's training front end read-only and writes what the Rust port must
reproduce: unidecode's fold per code point, op_g2p's normalized text per
probe, every word op_g2p sent to its neural graph with the IPA it got back,
and the canonical contraction tables (embedded by the Rust crate).

Run from the Prosodia repo root on ai-lab-0:

    ../Sonora/github/.venv/bin/python crates/actor/tests/fixtures/device_g2p/generate.py
"""

import hashlib
import importlib.metadata
import json
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[4]
SONORA = REPO.parent / "Sonora" / "github"
sys.path.insert(0, str(SONORA))
sys.path.insert(0, str(SONORA / "scripts" / "litert_export"))

import unidecode  # noqa: E402
import device_g2p  # noqa: E402
from matcha.text import op_g2p  # noqa: E402

RANGES = [(0x0080, 0x00FF), (0x0100, 0x017F), (0x0180, 0x024F), (0x2000, 0x206F),
          (0x2100, 0x214F), (0x2190, 0x21FF), (0x2600, 0x27BF), (0x1F000, 0x1FAFF)]


def sha256_file(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def write(path, obj):
    path.write_text(json.dumps(obj, indent=2, ensure_ascii=False) + "\n")


def main():
    commit = subprocess.run(["git", "-C", str(SONORA), "rev-parse", "HEAD"],
                            capture_output=True, text=True, check=True).stdout.strip()
    dirty = subprocess.run(["git", "-C", str(SONORA), "status", "--porcelain", "--",
                            "matcha/text", "scripts/litert_export"],
                           capture_output=True, text=True, check=True).stdout.strip()
    if dirty:
        sys.exit(f"Sonora's front end has uncommitted changes:\n{dirty}")

    chars = {f"{cp:04X}": unidecode.unidecode(chr(cp))
             for lo, hi in RANGES for cp in range(lo, hi + 1)}
    write(HERE / "fold.json", {"unidecode_version": importlib.metadata.version("unidecode"),
                               "sonora_commit": commit, "ranges": [[lo, hi] for lo, hi in RANGES],
                               "chars": chars})

    raw_tables = op_g2p.contraction_tables()
    canonical = json.dumps(raw_tables, ensure_ascii=False, indent=2, sort_keys=True)
    (REPO / "crates" / "actor" / "resources" / "g2p_contractions.json").write_text(canonical)
    tables = json.loads(canonical)
    probes = device_g2p.probe_sentences(tables)
    write(HERE / "normalize.json", {"sonora_commit": commit,
                                    "cases": [{"text": t, "normalized": op_g2p.normalize_for_tokens(t)}
                                              for t in probes]})

    host = op_g2p.OpenPhonemizerG2P(use_neural_oov=True, homographs=False)
    seen = {}
    neural_word = host._neural_word

    def recording(word):
        ipa = neural_word(word)
        seen[word] = ipa
        return ipa

    host._neural_word = recording
    for t in probes:
        host.phonemize(t)
    assets = Path(host.assets_dir)
    write(HERE / "neural.json", {"sonora_commit": commit,
                                 "sha256": {n: sha256_file(assets / n)
                                            for n in ("dp_g2p_matcha_fp16.tflite", "g2p_meta.json")},
                                 "contractions_sha256": hashlib.sha256(canonical.encode()).hexdigest(),
                                 "words": dict(sorted(seen.items()))})
    print(f"{len(chars)} code points, {len(probes)} probes, {len(seen)} neural words, Sonora {commit[:9]}")


if __name__ == "__main__":
    main()
