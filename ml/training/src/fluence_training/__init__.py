# SPDX-License-Identifier: Apache-2.0
"""Task-LoRA distillation pipelines (SPEC §5.D, D-5.1).

Will implement QLoRA distillation — teacher: a large model; student:
Gemma E4B/E2B — one LoRA per task (`rephrase`, `replies`, `expand`),
evaluated before/after on the harness (a LoRA ships only if it beats
prompt-only on the frozen test split).

This is the ML-language parallel track (PLAN §3): it starts after Phase 3
and feeds its artifacts to Phases 4+ without blocking them. This package
stays empty until then.
"""
