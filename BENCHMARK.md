# Benchmark

Exact-split accuracy of the split scorers on `data/bench/cases.tsv` (4,000
synthetic cases from held-out sentences, seed 42) and `data/hard_cases.tsv`.
Gate: ≥ 95% overall and every `must` hard case passing.

```sh
cargo run --release -p lf-train -- bench                 # ngram, embedded models
cargo run --release -p lf-train -- bench --scorer last   # Phase 1 baseline
cargo run --release -p lf-train -- bench --models DIR    # en.lfng + fa.lfng from DIR
```

Corpora: see `models/SOURCES.md`. Timings are the average time to build words
and score one case, in release mode on the development machine.

## Current result (committed models: order 4, prune 2)

```
scorer: ngram     cases: 4000
overall exact-split accuracy:   95.1%
by direction    us→ir 91.9%   ir→us 97.9%
by prefix k     0: 95.7%  1: 94.5%  2: 95.1%  3: 95.2%  4: 94.9%
by suffix s     1: 99.3%  2–3: 94.6%  4–6: 93.8%
hard cases      22/22 (must: 17/17)
avg time per case: 0.025 ms
```

```
scorer: last      cases: 4000
overall exact-split accuracy:   17.9%
by direction    us→ir 19.4%   ir→us 16.6%
by prefix k     0: 17.9%  1: 16.5%  2: 18.8%  3: 18.9%  4: 17.4%
by suffix s     1: 100.0%  2–3: 0.0%  4–6: 0.0%
hard cases      18/22 (must: 15/17)
avg time per case: 0.002 ms
```

## Runs

Cases **v1** = first generation. Cases **v2** = regenerated after `gen-bench`
started typing Persian text with Persian yeh/kaf (see the note below); v2 is
the committed set.

| Date | Cases | Scorer | Order | Prune | Overall | us→ir | ir→us | Hard (must) | en.lfng | fa.lfng | ms/case |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 2026-10-22 | v1 | last | – | – | 17.9% | 19.4% | 16.6% | – | – | – | 0.002 |
| 2026-10-22 | v1 | ngram | 3 | 1 | 93.1% | 88.3% | 97.4% | 22/22 (17/17) | 151 KB | 210 KB | 0.015 |
| 2026-10-22 | v1 | ngram | 3 | 2 | 93.2% | 88.3% | 97.4% | 22/22 (17/17) | 138 KB | 184 KB | 0.015 |
| 2026-10-22 | v1 | ngram | 3 | 3 | 93.3% | 88.5% | 97.5% | 22/22 (17/17) | 130 KB | 171 KB | 0.015 |
| 2026-10-22 | v1 | ngram | 4 | 1 | 93.8% | 89.1% | 97.9% | 22/22 (17/17) | 1.46 MB | 1.76 MB | 0.025 |
| 2026-10-22 | v1 | ngram | 4 | 2 | 93.9% | 89.3% | 97.9% | 22/22 (17/17) | 1.18 MB | 1.39 MB | 0.025 |
| 2026-10-22 | v1 | ngram | 4 | 3 | 93.8% | 89.3% | 97.8% | 22/22 (17/17) | 1.05 MB | 1.25 MB | 0.024 |
| 2026-10-22 | v2 | last | – | – | 17.9% | 19.4% | 16.6% | 18/22 (15/17) | – | – | 0.002 |
| 2026-10-22 | v2 | ngram | 3 | 1 | 94.5% | 91.2% | 97.4% | 22/22 (17/17) | 151 KB | 210 KB | 0.015 |
| 2026-10-22 | v2 | ngram | 3 | 2 | 94.5% | 91.2% | 97.4% | 22/22 (17/17) | 138 KB | 184 KB | 0.014 |
| 2026-10-22 | v2 | ngram | 3 | 3 | 94.6% | 91.3% | 97.5% | 22/22 (17/17) | 130 KB | 171 KB | 0.014 |
| 2026-10-22 | v2 | ngram | 4 | 1 | 95.0% | 91.8% | 97.9% | 22/22 (17/17) | 1.46 MB | 1.76 MB | 0.024 |
| **2026-10-22** | **v2** | **ngram** | **4** | **2** | **95.1%** | **91.9%** | **97.9%** | **22/22 (17/17)** | **1.18 MB** | **1.39 MB** | **0.024** |
| 2026-10-22 | v2 | ngram | 4 | 3 | 95.0% | 91.9% | 97.8% | 22/22 (17/17) | 1.05 MB | 1.25 MB | 0.023 |

Choice: order 4, prune 2, the best accuracy. Order 4 costs about 8× the size
of order 3 for about 0.5 points, and is still far under the 4 MB limit.

## Notes

**v1 → v2: Arabic yeh and kaf.** About 7% of the Persian corpus lines use
Arabic `ي`/`ك`, which Persian Standard puts on Shift+D / Shift+Z. v1 typed
them that way, so those cases showed `D`/`Z` in the us-typed text
(`ihD lhgD`), a keystroke Persian Standard users do not make for `ی`/`ک`.
Those 140 us→ir cases failed at 43% against about 6% for the rest. v2 types
the Persian letters (plain D and `;`) instead. The scorer and models did not
change between v1 and v2.

**Where the errors are.** Most misses are us→ir with short Persian function
words at the boundary, which are also plausible English letter strings:
`از`→`hc`, `را`→`vh`, `در`→`nv`, `به`→`fi`. A number of others contain digits
or punctuation inside the suffix (`۲۹.۵٪`, `)2(`), which score 0 and shift
the split by one word. ir→us is easier because typed English rarely looks
like Persian.

**Margin.** 95.1% clears the gate by 0.1 points. The cases and models are
fixed, so the CI result is deterministic, but any later change to the
models or scorer can tip it under. The next steps from spec section 11 are
OpenSubtitles text (needs the user to download it) and a top-20k word
frequency bonus (format version 2). Order 5 for Persian does not fit the
version 1 key, which holds at most 4 symbols.
