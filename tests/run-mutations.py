#!/usr/bin/env python3
"""Mutation harness for the reporter checks in src/stub_structs.rs.

Each mutation patches one line of the reporter and requires that the **named**
check fails. A mutation killed by some other check is reported as MISDIRECTED and
counts as a failure of this harness.

Two ways to run:

  ./tests/run-mutations.py
      Runs `cargo test` in this crate. Needs the toolchain in
      rust-toolchain.toml (1.82.0 with rustc-dev), because venir links against
      rustc internals through rust_verify.

  ./tests/run-mutations.py --bridge <dir>
      Transplants src/stub_structs.rs into an ordinary crate at <dir> -- one that
      depends on `air` and `vir` but not on `rust_verify` -- and runs the checks
      there. The `impl rust_verify::verifier::Diagnostics for Reporter` block is
      elided on the way in; it contains three empty methods and no check touches
      it. This is how the results recorded for VN-M5 were produced, because
      `rust_verify` cannot be built on macOS at all: it needs `rustc-dev` for a
      pinned x86_64-linux toolchain. Everything the checks exercise -- the
      message-to-wire mapping, including the counterexample branch -- is in the
      transplanted half.

Per Testing/Verification-Harness-Traps.md the verdict comes from the parsed
per-test result lines. A run that produces none is a harness failure, not a kill.
"""

import argparse
import re
import subprocess
import sys
from dataclasses import dataclass, field
from pathlib import Path

VENIR = Path(__file__).resolve().parent.parent
TARGET = "src/stub_structs.rs"


@dataclass
class Mutation:
    id: str
    find: str
    replace: str
    killer: str
    why: str = ""


MUTATIONS = [
    Mutation(
        "VM1",
        "                Some(model) => SmtOutput::Counterexample(CounterexampleBlock { model }),",
        "                Some(model) => {\n"
        "                    let _ = model;\n"
        "                    SmtOutput::Note(msgx.note.clone())\n"
        "                }",
        "v1_a_counterexample_note_is_emitted_as_a_counterexample",
    ),
    Mutation(
        "VM2",
        "                None => SmtOutput::Note(msgx.note.clone()),",
        "                None => SmtOutput::Counterexample(CounterexampleBlock {\n"
        "                    model: Default::default(),\n"
        "                }),",
        "v2_control_an_ordinary_note_stays_a_note",
    ),
    Mutation(
        "VM3",
        "            match air::model::counterexample_from_note(&msgx.note) {",
        "            match serde_json::from_str::<Counterexample>(&msgx.note).ok() {",
        "v3_an_unannounced_json_note_is_not_a_counterexample",
    ),
    Mutation(
        "VM4",
        "                error_block.error_span = extract_span_from_string(&span.as_string);",
        "                error_block.error_span = span.as_string.clone();",
        "v4_the_existing_wire_shapes_are_unchanged",
    ),
    Mutation(
        "VM5",
        "        Warning => SmtOutput::Warning(WarningBlock { warning_message: msgx.note.clone() }),",
        "        Warning => SmtOutput::Note(msgx.note.clone()),",
        "v4_the_existing_wire_shapes_are_unchanged",
    ),
    Mutation(
        "VM6",
        "        crash_span: extract_span_from_string(note),",
        "        crash_span: String::new(),",
        "v4_the_existing_wire_shapes_are_unchanged",
    ),
]

DECLARED_SURVIVORS = []

TEST_LINE = re.compile(r"^test (\S+) \.\.\. (ok|FAILED|ignored)")


def matches(reported: str, killer: str) -> bool:
    """`cargo test` prints unit tests as `tests::name` and integration tests as
    `name`; the mutation table names the check, not its module path."""
    return reported == killer or reported.endswith("::" + killer)


@dataclass
class RunResult:
    rc: int
    passed: list = field(default_factory=list)
    failed: list = field(default_factory=list)

    @property
    def total(self):
        return len(self.passed) + len(self.failed)


def transplant(bridge: Path):
    """Copy stub_structs.rs into the bridge crate, minus the rust_verify impl."""
    src = (VENIR / TARGET).read_text()
    start = src.index("impl rust_verify::verifier::Diagnostics for Reporter")
    end = src.index("impl Reporter {")
    body = (
        src[:start]
        + "// (rust_verify Diagnostics impl elided by tests/run-mutations.py --bridge)\n\n"
        + src[end:]
    )
    (bridge / "src" / "lib.rs").write_text(body)


def run_suite(bridge) -> RunResult:
    if bridge:
        transplant(bridge)
        cmd = ["cargo", "test", "-p", bridge.name]
        cwd = bridge.parent
    else:
        cmd = ["cargo", "test"]
        cwd = VENIR
    proc = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, timeout=1800)
    out = proc.stdout + proc.stderr
    res = RunResult(rc=proc.returncode)
    for line in out.splitlines():
        m = TEST_LINE.match(line.strip())
        if m:
            (res.passed if m.group(2) == "ok" else res.failed).append(m.group(1))
    if res.total == 0:
        print("---- suite produced no test result lines; last 40 lines follow ----")
        print("\n".join(out.splitlines()[-40:]))
    return res


def apply(mut: Mutation) -> str:
    path = VENIR / TARGET
    original = path.read_text()
    if original.count(mut.find) != 1:
        raise SystemExit(
            f"{mut.id}: pattern occurs {original.count(mut.find)} times, expected exactly 1. "
            f"The mutation table has drifted from {TARGET}."
        )
    path.write_text(original.replace(mut.find, mut.replace))
    return original


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--bridge", type=Path, default=None)
    args = ap.parse_args()
    bridge = args.bridge.resolve() if args.bridge else None

    print("== control ==")
    control = run_suite(bridge)
    if control.failed or control.total == 0:
        print(f"CONTROL IS NOT GREEN: rc={control.rc} failed={control.failed}")
        return 1
    print(f"control: rc={control.rc}, {len(control.passed)} tests, 0 failures")
    print(f"tests: {', '.join(sorted(control.passed))}\n")

    problems = 0
    for mut in MUTATIONS + DECLARED_SURVIVORS:
        original = apply(mut)
        try:
            res = run_suite(bridge)
        finally:
            (VENIR / TARGET).write_text(original)
        declared = mut in DECLARED_SURVIVORS
        if res.total == 0:
            verdict, note = "HARNESS-ERROR", "suite produced no results"
            problems += 1
        elif declared and res.failed:
            verdict, note = "NO-LONGER-A-SURVIVOR", f"now killed by {', '.join(res.failed)}"
            problems += 1
        elif declared:
            verdict, note = "survived (declared)", mut.why
        elif not res.failed:
            verdict, note = "SURVIVED", "no check noticed"
            problems += 1
        elif any(matches(f, mut.killer) for f in res.failed):
            others = [f for f in res.failed if not matches(f, mut.killer)]
            verdict = "killed"
            note = mut.killer + (f" (+{len(others)} more)" if others else "")
        else:
            verdict, note = "MISDIRECTED", f"died in {', '.join(res.failed)}, not {mut.killer}"
            problems += 1
        print(f"{mut.id:<5} {verdict:<14} {note}")

    if bridge:
        transplant(bridge)  # leave the bridge holding the unmutated source
    print(f"\n{problems} problems")
    return 0 if problems == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
