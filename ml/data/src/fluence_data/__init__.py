# SPDX-License-Identifier: Apache-2.0
"""Synthetic dialogue generation and corpus preparation (SPEC §5.D, D-5.5).

Implements the controlled generation matrix (12 situations × 4 registers,
strict anti-pathos instruction), the input-variant generators (telegraphic,
AZERTY spatial-confusion noise, abbreviations), automatic-judge filtering and
dataset hygiene (versioned JSONL, datasheets, frozen splits — the test split
never tunes prompts).

Phase 3 (PLAN task 3.1) lands the versioned corpus format; the corpus v0
itself (task 3.4) and the variant generators build on it.
"""

from fluence_data.antipathos import PATHOS_MARKERS, is_pathos_free, pathos_findings
from fluence_data.azerty import (
    confusion_distribution,
    neighbours,
    sample_keypress,
)
from fluence_data.corpus_v0 import build_corpus_v0
from fluence_data.formats import (
    SCHEMA_VERSION,
    Dialogue,
    InputVariant,
    Register,
    Situation,
    Speaker,
    Split,
    Turn,
    VariantKind,
    dump_jsonl,
    load_jsonl,
    split_of,
)
from fluence_data.variants import abbreviated, noised, telegraphic

__all__ = [
    "PATHOS_MARKERS",
    "SCHEMA_VERSION",
    "Dialogue",
    "InputVariant",
    "Register",
    "Situation",
    "Speaker",
    "Split",
    "Turn",
    "VariantKind",
    "abbreviated",
    "build_corpus_v0",
    "confusion_distribution",
    "dump_jsonl",
    "is_pathos_free",
    "load_jsonl",
    "neighbours",
    "noised",
    "pathos_findings",
    "sample_keypress",
    "split_of",
    "telegraphic",
]
