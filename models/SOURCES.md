# Model sources

The models are built offline by `lf-train` from these corpora. The corpus
files themselves are not committed (`data/corpus/` is gitignored).

| Model | Corpus files (Leipzig Corpora Collection) | Lines after `prepare` |
| --- | --- | --- |
| `en.lfng` | `eng_news_2020_1M-sentences.txt`, `eng_newscrawl-public_2018_1M-sentences.txt` | 1,000,000 (of 2M, shuffled with seed 42) |
| `fa.lfng` | `pes-ir_web_2019_1M-sentences.txt`, `pes_wikipedia_2021_1M-sentences.txt` | 1,000,000 (of 2M, shuffled with seed 42) |

Source: Leipzig Corpora Collection, <https://wortschatz.uni-leipzig.de/en/download>.

Licence: the terms of usage (<https://wortschatz.uni-leipzig.de/en/usage>)
state that the downloadable text corpora are available under Creative
Commons **CC BY**. *Not yet checked against the live terms page (it was
unreachable from the build machine); confirm before redistributing.*

Requested citation:

> D. Goldhahn, T. Eckart, U. Quasthoff: Building Large Monolingual
> Dictionaries at the Leipzig Corpora Collection: From 100 to 200 Languages.
> Proceedings of LREC 2012.

## Rebuilding

```sh
lf-train prepare --lang en --in data/corpus/en --out data/clean/en.txt
lf-train prepare --lang fa --in data/corpus/fa --out data/clean/fa.txt
lf-train train --lang en --in data/clean/en.txt --out models/en.lfng --order 4 --prune 2
lf-train train --lang fa --in data/clean/fa.txt --out models/fa.lfng --order 4 --prune 2
```

`prepare` folds curly apostrophes (`’ ‘`) to `'` in English text, since
news text writes `don’t` but people type `don't`.
