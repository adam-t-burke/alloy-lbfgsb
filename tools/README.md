# Driver profiling tools

These are investigation tools, not an alternative production solver. Use uv for
Python; no Python packages beyond the standard library are required. Windows and
Linux affinity are supported. Select an allowed logical CPU with --cpu (default 2).
No sampling session is started and no other processes have their affinity changed.

profile_bench.rs is the frozen common harness derived from 9a200cb. Every variant
uses this same source, Cargo.lock, features and release settings. The measured
invocation includes input reset and common coordinate/gradient fingerprints.
Each run warms reusable solver storage first. Source patches under variants/
record exact experiments; they are not production compatibility implementations.

Reproduction in a checkout with no existing target/profile-* worktrees:

```text
uv run --no-project tools/profile_driver.py --prepare --variants original,current,no-fill,reuse,paired,isolated,equivalent-original,direct,inline
uv run --no-project tools/profile_driver.py --build
uv run --no-project tools/profile_driver.py --run --repeats 5 --millis 750
uv run --no-project tools/profile_driver.py --build --profile
uv run --no-project tools/profile_driver.py --run --profile --repeats 2 --millis 500
```

Preparation refuses to overwrite existing experiment directories. Original and
equivalent-original use 9a200cb; other variants use ca6baa5 plus their patch.
Use --variants and --label to build/run a separate comparison; for example:

```text
uv run --no-project tools/profile_driver.py --build --variants original,isolated --label final
uv run --no-project tools/profile_driver.py --run --variants original,isolated --label final --repeats 5 --millis 750
```

--cases selects dimensions from the fixed matrix; --backends selects deterministic
and/or faer. Variants rotate order per repetition. Every completed comparison
checks final coordinate/gradient bit fingerprints, values and counters within each
backend/case. Instrumented binaries have separate manifests and timing records.
Absolute executable manifests are ignored. Raw JSON records are versioned.
A rerun with the same label overwrites that result; use a new label to preserve it.

summarize_driver.py reads the committed experiment labels, checks cross-experiment
fingerprints and independently recomputes medians/percentages with decimal arithmetic.
For a new experiment label, add its reference variant to REFERENCES before summarizing.
It compares percentages with an absolute tolerance of 1e-10 percentage points.

```text
uv run --no-project tools/summarize_driver.py
```

The selected library code corresponds to isolated, with additional comments/tests.
Rejected controls remain reproducible. Delete only your generated experiment
worktrees with git worktree remove after preserving needed results. No automatic
recursive cleanup is performed by the tools.
