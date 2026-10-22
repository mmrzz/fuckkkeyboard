# Benchmark

Exact-split accuracy of the split scorers on `data/bench/cases.tsv` (4,000
synthetic cases from held-out sentences, seed 42) and `data/hard_cases.tsv`.
Gate: ≥ 95% overall and every `must` hard case passing.

```sh
cargo run --release -p lf-train -- bench                 # ngram, embedded models
cargo run --release -p lf-train -- bench --scorer last   # Phase 1 baseline
cargo run --release -p lf-train -- bench --models DIR    # en.lfng + fa.lfng from DIR
cargo run --release -p lf-train -- bench --alpha 0       # word-frequency bonus off
cargo run --release -p lf-train -- bench --failures 200  # list misses
```

Corpora: see `models/SOURCES.md`. Timings are the average time to build words
and score one case, in release mode on the development machine.

## Current result (committed models: format v2, order 4, prune 2, top-20k words)

The gate is 95%, and 97% is the target so later changes have headroom. The
current result is **96.95%** (3878/4000), which the report rounds to 97.0%:
2 cases short of the target.

```
scorer: ngram     cases: 4000
word bonus      alpha 1.5 cap 4 floor 2 ppm
overall exact-split accuracy:   97.0%  (3878/4000)
by direction    us→ir 95.8%   ir→us 98.0%
by prefix k     0: 97.9%  1: 96.4%  2: 96.4%  3: 97.3%  4: 96.6%
by suffix s     1: 99.3%  2–3: 96.9%  4–6: 96.1%
hard cases      38/38 (must: 27/27)
avg time per case: 0.029 ms
```

```
scorer: last      cases: 4000
overall exact-split accuracy:   17.9%  (716/4000)
by direction    us→ir 19.4%   ir→us 16.6%
by prefix k     0: 17.9%  1: 16.5%  2: 18.8%  3: 18.9%  4: 17.4%
by suffix s     1: 100.0%  2–3: 0.0%  4–6: 0.0%
hard cases      32/38 (must: 24/27)
avg time per case: 0.002 ms
```

## Word-frequency bonus and camelCase fix: before and after

All on cases v2, order 4, prune 2, with the 38 hard cases in the file now.

| Step | Overall | us→ir | ir→us | Hard (must) |
| --- | --- | --- | --- | --- |
| Before: format v1 models, spec 2.3 camelCase rule | 95.1% | 91.9% | 97.9% | 37/38 (26/27) |
| + top-20k word lists, bonus α 1.5 cap 4 | 95.3% | 92.3% | 98.0% | – |
| + best bonus found without the camelCase fix (α 3, cap 20) | 95.6% | 92.9% | 98.0% | – |
| camelCase fix alone (bonus off) | 96.88% (3875) | 95.7% | 97.9% | 38/38 (27/27) |
| **After: camelCase fix + bonus α 1.5 cap 4** | **96.95% (3878)** | **95.8%** | **98.0%** | **38/38 (27/27)** |
| Experiment, not adopted: letterless words join the converted suffix | 97.33% (3893) | 97.9% | 96.9% | 37/38 (26/27) |

Bonus grid (with the camelCase fix): α ∈ {0.25, 0.5, 1, 1.5, 2, 3} ×
cap ∈ {2, 4, 6, 8, 12, 20} × floor ∈ {2, 10} ppm. The best settings get
3878 correct against 3875 with the bonus off, so the bonus is worth 3 cases
here.
α 1.5 cap 4 floor 2 ties for best and keeps the bonus graded: English `hc`
(2.4 per million, rank 17,843) gets 0.4 bits, `nv` is not in the list at
all, and Persian `در` (38,700 per million) gets the full 4 bits.

Model sizes with the word list: en.lfng 1.34 MB, fa.lfng 1.55 MB (+160 KB
each). Scoring 256 keys takes 0.27 ms (was 0.17 ms).

The word lists are counted over each corpus's full 1M sentences, which
include the 2% the benchmark is drawn from. For top-20k words this barely
matters (a word cannot reach the list from the held-out part alone), but
it is a small leak.

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

These runs predate the word list and the camelCase fix; hard-case counts
are out of the 22 cases the file had then.

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

**camelCase pins Persian typed on US (fixed).** Persian Standard puts
letters on Shift (`آ` Shift+H, `ئ` Shift+S, `ء` Shift+M) and tanwin on
Shift+R, so Persian typed on US often has a lowercase letter followed by an
uppercase one: `درآمد` is `nvHln`, `مسلماً` is `lsglhR`. The spec 2.3 rule
pinned these at −50, so they were never converted: 74 of the 186 misses
that were left after adding the bonus. The rule now skips a hump when the
uppercase key types a letter in the target layout, or when it is the last
char (word-final tanwin). `getUserName`, `iPhone` and `myVar` stay pinned,
because their humps type marks, not letters. `getData` and `setState` are
no longer pinned, and the LLR keeps them English (hard cases).

An earlier version of this file said most misses were short function
words. That was wrong: the camelCase pin was the largest cause.

**Where the remaining errors are.** 68 of the 122 misses have a boundary
word with no letters as typed (`)۲(`, `۱۳`, `3@604`, `96,`). In 62 of
them it scores 0, and the spec 2.2 tie rule (ties go to the later word:
convert less) leaves it on the unconverted side, though every letter is
still converted correctly. In the other 6 the word gains a letter when
rendered (`96,` becomes `۹۶و`, since the comma key types `و`), so it is
scored as Persian and converted with the suffix one word too early. Making such words join the converted suffix gives 97.33%, but it
breaks the `123 lvsd` must case (which encodes the tie rule) and costs ir→us
a point, so it is not adopted. 27 misses are one short word at the boundary
(English `A`/`an` looking like Persian `ش`/`شد`, or short Persian words),
14 are one longer word, 7 mix the two, and 6 are off by 2 or more words.

**Margin.** 96.95% is 1.95 points above the 95% gate. The cases and models
are fixed, so CI is deterministic. Order 5 for Persian does not fit the
key, which holds at most 4 symbols.

## Remaining misses (122)

The current scorer on cases v2, grouped by what sits between the expected
and the chosen split.

### Boundary word has no letters as typed (68: 62 split too late, 6 too early)

| Layout | Typed | Expected | Got |
| --- | --- | --- | --- |
| us | `Esther E. Blocher, 96, vh oak,n j,hkn 'vnhkdn.` | 4 | 3 |
| ir | `به )۲( لهرثس فاث ژخپپهسسهخد فاث حخصثق` | 1 | 2 |
| ir | `این در حالی ۱۳ سفشفثس فاشف قثزثدفمغ قثزثهرثی بثیثقشم` | 3 | 4 |
| us | `To help keep this 3@604 ktv [ludj` | 4 | 5 |
| ir | `، ثرثقغفاهدل \u{651}دیهش` | 0 | 1 |
| us | `Starting November 9, 2015, shg 1293r vh nv Hk 'vnH,vni.` | 4 | 2 |
| us | `75 lsglhk hc lndki [ij p[` | 0 | 1 |
| us | `The Rev. Soto & o,hsjhv` | 3 | 4 |
| ir | `نیهمردم ۵۵۵ نفر ۲۹.۵٪ خب فاث حخحعمشفهخد.` | 3 | 4 |
| ir | `تا دادگاه ۳۲۵۸ \u{64d}. إء` | 2 | 3 |
| ir | `پدرش آلپ ۸ پهممهخد فخدس` | 2 | 3 |
| us | `The 930 ktv lhidhki hc wkn,r hujfhvd` | 1 | 2 |
| us | `)2( ukhwv nv iv sxg lvjf` | 0 | 1 |
| ir | `گزارش شده ۲۰۰۰ أعهدثشس سعززثسس خد »شپثنخ` | 2 | 3 |
| us | `By doing & 94% hc 388 krn` | 2 | 4 |
| ir | `چند روز ۹ سثف عس ذشزن شلشهد` | 2 | 3 |
| us | `I was " vtdr` | 2 | 3 |
| us | `On March 1, ;i lk o,n nv nhka mdhnihl.K` | 3 | 2 |
| us | `202 ktv )44ohk,hv(` | 0 | 1 |
| us | `Disbursements will 2219 fi ugj o,hw lkhsf jv&` | 2 | 3 |
| us | `Executive 9 jsh,d , 9 a;sj` | 1 | 2 |
| us | `Malaika & [ludj Hk 206 ktv )40ohk,hv(` | 1 | 2 |
| us | `Round 5, ;i jth,j ihd` | 2 | 1 |
| ir | `۹ هدزاثس فشمم شدی ۱۵۰ حخعدیس.` | 0 | 1 |
| us | `14 h;jfv& jlhl ;a,vihd hjphndi` | 0 | 1 |
| us | `1949 fv'cdni` | 0 | 1 |
| ir | `به سبب ۶۰ سخعفا هد خقیثق فخ` | 2 | 3 |
| us | `In recent years 140 uq, nhvn , l[gs lgd` | 3 | 4 |
| us | `However, Sykes 10 shg'd` | 2 | 3 |
| us | `Marcus Rashford 3@659 ktv [ludj` | 2 | 3 |
| us | `The dashboard also allows 7 gd,hk` | 4 | 5 |
| ir | `معمولا\u{64b} در ترکیب با ۶ ح.پ. فاث فاهقی` | 4 | 5 |
| us | `Never will 2020 tdgl fv'vd fhf lkjav o,hin` | 2 | 3 |
| us | `It really 1313 fi ilnhk Hlni , fi` | 2 | 3 |
| us | `2013& lhv; ,f nv j,ddjva k,aj` | 0 | 1 |
| us | `Hugh joined 2010 fh h, nv hvjfhx f,n.` | 2 | 3 |
| us | `I'm voting for 11& alhvi` | 3 | 4 |
| us | `Further, I feel 1@629 ktv [ludj nhvn.` | 3 | 4 |
| us | `Add to that $1,000 rhk,k` | 4 | 3 |
| ir | `قهرمان ۶۰ حثقزثدف` | 1 | 2 |
| us | `The consultation for Alison 2 lohgt , 1` | 4 | 5 |
| us | `I'll look it up 314 ktv` | 4 | 5 |
| us | `The First Cobalt Refinery & [ludj Hk 146 ktv` | 4 | 5 |
| us | `10 ldgd,k ktv [ludj wpfj` | 0 | 1 |
| ir | `همچنین ۱۳ پهمثس.` | 1 | 2 |
| us | `Human & xvhpd m,sji , kwf m,vj.` | 1 | 2 |
| us | `It's apparently & avt` | 2 | 3 |
| us | `In the past, Brown & [ludj Hk 8487 ktv` | 4 | 5 |
| us | `Loving husband of 42 1 fsdhv ;ljv hsj" ,gd nv` | 4 | 5 |
| ir | `۱۱۵-۱۰۶ خد` | 0 | 1 |
| us | `: nvw,vj` | 0 | 1 |
| us | `She )1784( d; ;ajd f,n` | 1 | 2 |
| us | `Over the years, I 4065 ktv` | 4 | 5 |
| us | `- [hSti -` | 0 | 1 |
| us | `Before Monday, Wall Street 813 ljv fhghjv hc` | 4 | 5 |
| ir | `مشاورت استاندار ۱۴.۴ پپ خب قشهدبشمم صشس` | 2 | 3 |
| ir | `سال ۲ خد` | 1 | 2 |
| ir | `گرگورهای بزرگ را در ۲۵۰ شدی ۲۰۰ پهممهخد غثشقس` | 4 | 5 |
| us | `Two more 'Full Houses' & [ludj Hk 146 ktv )47ohk,hv(` | 4 | 5 |
| us | `YouTube is rife with - nhka'hi ug,l mca;d ,` | 4 | 5 |
| us | `And it 20 lvnhn` | 2 | 3 |
| ir | `روی ۱۶ فهپثس پشلدهبهزشفهخد قشدلث ثضعهرشمثدف` | 1 | 2 |
| us | `On Monday, January 6, o,vnk ',aj lvnhv` | 4 | 3 |
| us | `Anger at & ;i funih lua,riN agjk an&` | 2 | 3 |
| us | `Yesterday I posted the 1972 hc ,d [nh an.` | 4 | 5 |
| us | `Al-Thuwaikh was speaking at 2010 jh phgh` | 4 | 5 |
| us | `With 50 seconds to 1@634 ktv [ludj nhvn.` | 4 | 5 |
| us | `I have 2_ Hkih hc phtzi fsdhv o,fd` | 2 | 3 |

### Boundary mixes digits or punctuation with words (7)

| Layout | Typed | Expected | Got |
| --- | --- | --- | --- |
| us | `In 2008, Munoz a,n , dh` | 3 | 1 |
| us | `U think any aY;tjd & ;i iv aUf afY` | 3 | 8 |
| us | `, LsYtQv aTi,nK hahvi` | 0 | 3 |
| us | `Mohammed Akhtar, 50, of ;i tgc` | 4 | 2 |
| ir | `۸-۱۶: ؤ قثپهدیثق مثففثق شدی` | 0 | 2 |
| ir | `« ماده أخی دخذخیغ صشس سثقهخعسمغ هدتعقثی!` | 2 | 0 |
| ir | `« تعلیق عقد مهبث.` | 3 | 0 |

### One short word (2 letters or fewer) at the boundary (27)

| Layout | Typed | Expected | Got |
| --- | --- | --- | --- |
| us | `Thankfully, an fcv' jv ndni` | 2 | 1 |
| us | `A hcn,h[ ;kd?` | 1 | 0 |
| us | `A k;kdl` | 1 | 0 |
| ir | `درجه تک-ستاره دارای [اي خد يهزنثدسو ذعف هف` | 3 | 4 |
| us | `A n,gj hkjrhgd fi uk,hk ,hgd mv,hk` | 1 | 0 |
| ir | `الهام گرفت، شد شقثش خبب فاث` | 2 | 3 |
| us | `A ;i nv k,v,c 1378 hc fvkhli` | 1 | 0 |
| ir | `عرض مش شقث` | 2 | 1 |
| ir | `بنابر این سم ؤ: \u{64f}اهس هس ش` | 3 | 4 |
| ir | `فروش شماره مجازی شد هیثش غخع زشد عسث.` | 3 | 4 |
| us | `hb ghp[J ugdi lk urg` | 0 | 1 |
| us | `A ilvhi sd nd w,jd hvhSi ani` | 1 | 0 |
| us | `They have more capacity ;nd ;i nv fdk ;nihd fhc` | 4 | 5 |
| ir | `من به نمایندگی از شد شزف` | 4 | 5 |
| ir | `کل مجموعه، چیزی در شد شپذعمشدزث اشی ذثثد سثدف بقخپ` | 4 | 5 |
| us | `A lk vh Hchn ;kdn.` | 1 | 0 |
| us | `'A tgaf; low,w lg;id o,kdk.` | 1 | 0 |
| us | `A vh hd[hn ;vn hnhli dhtj.` | 1 | 0 |
| us | `A lnjd` | 1 | 0 |
| ir | `شد عدعسعشممغ اهلا` | 0 | 1 |
| us | `We're an jG;dn nhaj` | 2 | 1 |
| us | `A kihdj nv mC,ia` | 1 | 0 |
| ir | `دانشجویان و )شد خززشسهخدشممغ سخپث اثشقفذقثشن( شدی \u{651}` | 2 | 3 |
| us | `A , dh nd'v o,hihki ulg` | 1 | 0 |
| us | `A nhvhd lvnlhkd livfhk , Hvhl ,` | 1 | 0 |
| us | `He's a FA hkshk ]knhk` | 3 | 2 |
| us | `A hlv fh` | 1 | 0 |

### One longer word at the boundary (14)

| Layout | Typed | Expected | Got |
| --- | --- | --- | --- |
| ir | `و کوینت کورس اثگس ۴۵ غثشقس خمیو ذعف اث` | 3 | 2 |
| us | `Nelson pressured the ;k;had mdvhl,k` | 3 | 4 |
| ir | `و در دره فوکیس. \u{64c}شظهق شمسخ یهسزمخسثی فاشف بخقثهلد` | 4 | 3 |
| ir | `و «پ۹.۹ پهممهخد پخقث فاشد فاث` | 1 | 2 |
| us | `isn lh shdj thvsd l,cd;` | 0 | 1 |
| ir | `هم قسمت شدی/خق صهفا زخ-پخقذهیهفهثس( شدی` | 2 | 3 |
| ir | `تا ۴۰و شححثشقثی` | 1 | 2 |
| ir | `تحقیق ءشط \u{64c}اهففشنثق/ئهمن` | 1 | 2 |
| us | `GOPIO & hdk]i sudnkzhl 133 ktv` | 1 | 0 |
| ir | `خوردن ۲۰۱۹و صهفا آغحخحمشسفهز «ثبف آثشقف ئغدیقخپثو` | 1 | 2 |
| us | `Precedents have ado)vi(: Llsjpf hsj ;i` | 2 | 3 |
| ir | `بعدا\u{64b}، رقیه همچنین ۲۴۴و۰۰۰ حثخحمث صخقمیصهیث.` | 3 | 4 |
| ir | `سیستم رهسهف اففح://شبقهزشدمثشیثقساهحشزشیثپغ.` | 1 | 2 |
| us | `Separately, Deutsche Bank jhWsdshj isjkn"` | 3 | 4 |

### Off by 2 or more words (6)

| Layout | Typed | Expected | Got |
| --- | --- | --- | --- |
| ir | `میانگین سطح سواد در ش \u{650}\u{64e}\u{64e}\u{64e}أ\u{64d} ثلخو صخعمی` | 4 | 6 |
| ir | `ؤعلعسف ۱۰و ۲۰۲۰.` | 0 | 2 |
| ir | `بر اساس کتاب ءشغ ۲۲و فاث` | 3 | 5 |
| us | `During its first-ever v,n lvIi odg nv Hk` | 3 | 5 |
| us | `Dallas is the ,U jToQvY[T hgQlUdIYjU lYkU hgQpUdIYK"` | 3 | 7 |
| ir | `شایان ذکر ءشقزا ۳۱و ۲۰۲۰و ساخصهدل یخعذمث-یهلهف هپحقخرثپثدف` | 2 | 5 |
