# Model sources

The models are built offline by `lf-train` from these corpora. The corpus
files themselves are not committed (`data/corpus/` and `data/words/` are
gitignored).

| Model | Corpus files (Leipzig Corpora Collection) | Lines after `prepare` |
| --- | --- | --- |
| `en.lfng` | `eng_news_2020_1M-sentences.txt`, `eng_newscrawl-public_2018_1M-sentences.txt` | 1,000,000 (of 2M, shuffled with seed 42) |
| `fa.lfng` | `pes-ir_web_2019_1M-sentences.txt`, `pes_wikipedia_2021_1M-sentences.txt` | 1,000,000 (of 2M, shuffled with seed 42) |

The frequent-word lists (format version 2, top 20,000 words each) come from
the `*-words.txt` files in the same four archives, extracted to
`data/words/{en,fa}/`:

| Model | Word files | Tokens counted |
| --- | --- | --- |
| `en.lfng` | `eng_news_2020_1M-words.txt`, `eng_newscrawl-public_2018_1M-words.txt` | 38,392,665 |
| `fa.lfng` | `pes-ir_web_2019_1M-words.txt`, `pes_wikipedia_2021_1M-words.txt` | 37,627,229 |

Counts are merged after normalization (`NV` and `nv` are one word).
Multi-word entries and entries with letters outside the language are left
out.

Source: Leipzig Corpora Collection, <https://wortschatz.uni-leipzig.de/en/download>.

Licence: the downloadable text corpora are available under Creative Commons
**CC BY**, per the terms of usage at
<https://wortschatz.uni-leipzig.de/en/usage>. Verified against the live terms
page on 2026-10-22.

Requested citation:

> D. Goldhahn, T. Eckart, U. Quasthoff: Building Large Monolingual
> Dictionaries at the Leipzig Corpora Collection: From 100 to 200 Languages.
> Proceedings of LREC 2012.

## Rebuilding

```sh
lf-train prepare --lang en --in data/corpus/en --out data/clean/en.txt
lf-train prepare --lang fa --in data/corpus/fa --out data/clean/fa.txt
lf-train train --lang en --in data/clean/en.txt --out models/en.lfng --order 4 --prune 2 --words data/words/en
lf-train train --lang fa --in data/clean/fa.txt --out models/fa.lfng --order 4 --prune 2 --words data/words/fa
```

`prepare` folds curly apostrophes (`’ ‘`) to `'` in English text, since
news text writes `don’t` but people type `don't`.
