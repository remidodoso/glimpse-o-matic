# Technical Feedback — Watermarking Core Review

*From the white-paper author session, 2026-06-10. Audience: the pair-programming session.
Scope: assessment of the watermarking design's soundness, plus ranked, measure-first
proposals for tuning experiments and small fixes noticed while reading. Nothing here is a
demand; everything is a hypothesis with a suggested measurement. Sections 1–2 are
assessment; 3 is the ranked proposal list; 4 is the wavelet (CDF 9/7) question; 5 is
threat-model-scoped test-envelope gaps; 6 is small finds.*

---

## 1. Overall assessment: the approach is sound, and the observed robustness is predicted, not lucky

The architecture is a competent synthesis of established techniques, assembled in the
correct order:

- **Additive spread-spectrum** embedding (Cox et al. lineage) in mid-band DWT detail
  coefficients;
- **Code-division multiplexing** of the 192 payload bits — each bit owns a keyed ±1 PN
  tile (`pn_tile`, watermark.rs), all 192 summed into one weighted tile and added
  everywhere;
- **Tiled self-synchronization** (64×64 modulo tiling) for crop robustness;
- **Whitened-autocorrelation scale recovery** + **folded matched filter** for offset —
  structurally the same self-referencing sync approach the commercial (Digimarc-family)
  and academic literature converged on;
- **BCH(192,160) t=4 + CRC-32** layered the right way: ECC proposes, CRC disposes, so the
  false-accept probability stays ~2⁻³² and a clean read is never disturbed.

None of this is exotic, and that is a compliment. The measured robustness follows
directly from the structure:

- **Crop robustness is the tiling.** Any surviving rectangle contains whole PN periods;
  crop costs *area* only, and decode SNR degrades gracefully as √area. Unmarked content
  (viewer chrome, gray bars) is zero-mean against the PN and merely dilutes.
- **Recompression robustness is band placement + processing gain.** Levels [2,3] sit
  above the image's 1/f energy and below the frequencies JPEG kills first; each bit
  decision integrates over ~10⁵ coefficients (√N correlation gain), so per-coefficient
  damage must be enormous before sign decisions flip. The observed "~1 ECC bit per
  additional lossy hop" is exactly what this model predicts.
- **The complexity is allocated to the right side of the asymmetry.** The embedder (runs
  per-view, in a browser, possibly on a phone) is one DWT round-trip and an add. All the
  expensive machinery — FFTs, scale pyramid, 192-template bank — lives in the decoder,
  which runs rarely, offline, on a workstation. A shift-invariant embedding domain
  (undecimated DWT, Fourier-magnitude) would have avoided the registration problem
  entirely but at embed-time cost; pushing the pain to decode time was the right trade
  for this application.

**The one structural trade worth naming:** the critically-sampled DWT is shift-variant,
which is why scale must be recovered to <0.25%, why the blind search exists at all, and
why decode takes seconds. The consequence was engineered around honestly (pyramid,
harmonic siblings, refine ladder), but it is where most of the remaining fragility
lives, and most of the proposals below attack it or its margins.

---

## 2. Where the remaining failures live

From `blind_auto_sweep` + the floor measurements: the system fails at (a) the **source-size
floor** (~896 px clean matched floor; blind finder gives up earlier), and (b) the
**quality+crop cliff** (q40/q50 + heavy crop overruns t=4). Both are *margin* failures —
the signal is present but the integrated SNR or the residual bit-error count is just past
the budget. That means margin-recovery techniques (items 3.1–3.4 below) act exactly at
the boundary, where each dB of recovered margin moves the envelope, not in the interior
where everything already passes.

---

## 3. Ranked proposals (measure-first; each = hypothesis + cheap diagnostic)

### 3.1 Fold the partial periods — likely free SNR exactly at the size floor

`fold_tile` (registration mod) truncates to whole FOLD=512 multiples:

```rust
let (tw, th) = ((w / FOLD) * FOLD, (h / FOLD) * FOLD);
```

A 896-px source therefore contributes only its top-left 512² — **~33% of its pixels** —
to the fold; a 1023-px source contributes ~25%. Accumulating *every* pixel into its fold
position (`(y % FOLD, x % FOLD)`) uses 100% of the area. Positions then have unequal
accumulation counts (1..4), which the matched filter tolerates — it's mildly suboptimal
weighting (the correlation peak location is unchanged; noise becomes slightly
non-uniform), not corruption. Optional refinement: track a per-position count and either
normalize by it or weight the template correlation by it (this is the proper
maximal-ratio fold).

- **Hypothesis:** the ~896 px floor exists partly because most of a near-floor image is
  discarded; full accumulation buys up to ~1.7× amplitude there and pushes the floor down
  meaningfully.
- **Measurement:** re-run the source-size floor sweep (the one that found 896-pass /
  768-fail) with full-accumulation fold; report the new floor. ~10-line change, one
  existing sweep.
- **Caveat to check:** the worst case for unequal counts is a suspect just over one
  period; verify prominence doesn't degrade on *large* suspects (it shouldn't — counts
  become nearly uniform as area grows).

### 3.2 Soft-decision (Chase-style) decoding — the notes' own follow-on, endorsed concretely

`register_decode` computes per-bit correlation magnitudes (`maps[b][bp]`) and keeps only
signs. Near the cliff, bit errors should concentrate in the lowest-|corr| bits. Chase
decoding exploits this: take the k least-confident bits (k≈8), try all 2^k flip patterns
through BCH+CRC (each trial is microseconds), accept the first CRC-verified result.
Effective correction extends from t=4 to ~t+k/2-ish at the margin — plausibly converting
some q40/q50+heavy-crop failures.

- **Diagnostic first (cheap, run before implementing):** on the failing sweep cells,
  decode at the known-true scale, dump (bit-error position, confidence rank). If errors
  really are the low-|corr| bits, Chase will work and you can size k from the data. If
  they're *not* concentrated there, that's an interesting finding by itself (suggests
  structured interference rather than noise-limited errors).
- **Design notes:** CRC remains the only oracle (false-accept still ~2⁻³² per trial ×
  2^k trials = still negligible at k=8). Order trial patterns by ascending total
  flipped-confidence so the first verify is also the most plausible. Keep it
  decode-path-only — no WASM/embed impact, no channel break.

### 3.3 Sub-bin (parabolic) interpolation of the autocorrelation peak — attacks the notch/quantization mismatch

`scale_precision.md` records the awkward geometry: the 0-error registration notch is
<0.25% wide, but autocorr lag is integer-quantized (~0.4% at period 256) and the refine
step is 0.5% — blind success partly depends on a rung happening to land in the notch.
Integer lags are not the information limit: a quadratic fit through `prof[lag-1..=lag+1]`
in `scale_peaks` yields sub-bin peak position essentially free, plausibly ~10× finer
scale estimates.

- **Hypothesis:** interpolated candidate #1 lands inside the notch directly in most
  cases — fewer refine rungs fired, faster *and* more reliable blind decode.
- **Measurement:** for every sweep cell with known true scale, compare
  |interpolated-lag scale − true| vs |integer-lag scale − true|. Pure diagnostic, no
  decoder change needed to measure.
- **Note:** whitening sharpens the peak, which makes the 3-point parabola well-behaved;
  if a peak is exactly at a profile boundary, fall back to the integer lag.

### 3.4 Orthogonal codes (Hadamard × keyed scrambler) instead of 192 independent PN tiles

With 192 independent random ±1 tiles of length 4096, each bit's correlator carries
multi-access interference from the other 191: each interferer contributes ~√4096/4096 ≈
1.6% of signal amplitude; 191 of them sum to ~√191·1.6% ≈ **21% of signal amplitude
(1σ) of self-noise**, before the image or the channel contributes anything. Since
4096 = 2¹² and we only need 192 codes, replacing the tiles with **Walsh–Hadamard rows
scrambled by a single keyed PN sequence** gives exactly zero cross-correlation at aligned
offsets:

- keying is preserved (the scrambler is generated from WM_KEY; scrambled Hadamard rows
  are statistically white to anyone without the key);
- crop behavior is identical (orthogonality holds at the aligned fold offset, which is
  the only place bits are read; under misalignment scrambled-Hadamard behaves like
  random, i.e. no worse than today);
- the change is confined to `pn_tile` (and is a **channel break** — see 4.3).

- **Hypothesis:** every bit's margin rises by removing the 21% self-noise floor; this
  matters only at the cliff, which is where the interesting failures are.
- **Measurement:** histogram per-bit correlation margins, random vs orthogonal, on
  identical channel conditions (a handful of sweep cells near the cliff). If the margin
  distributions barely move, the image+channel noise dominates and this isn't worth the
  generation break; if the left tail tightens visibly, it is.

### 3.5 Maximal-ratio combining across subbands (minor)

`correlate_embed_levels` sums per-subband correlations with equal weight. The four
subbands (LH/HL × levels 2,3) have different post-channel SNRs (JPEG hits level-2 detail
harder than level-3; some images have anisotropic detail). Weighting each subband's
contribution by an estimate of its reliability (e.g., its correlation-magnitude
dispersion across the 192 bits, or subband energy ratios) is textbook MRC. Probably a
small win; cheap to measure by recomputing decisions offline from per-subband
correlations on the failing cells.

### 3.6 ALPHA tuning rides the Goldilocks figure

The white paper's planned too-weak/Goldilocks/too-strong sweep, if emitted with decode
*margin* (mean |corr| over threshold) rather than just pass/fail, doubles as the tuning
curve: margin and PSNR vs ALPHA on one plot shows how much invisible headroom 0.15 has.
Likely follow-on: per-image global strength adaptation (target fixed margin or fixed
PSNR), which the masking already half-does locally but not globally. (Coordinate with the
paper session — it needs a parameterized-ALPHA test-only embed anyway.)

---

## 4. The wavelet question (Haar → 5/3 → 9/7?)

### 4.1 Why the texture changed: the wavelet is the font the watermark is printed in

Every embedded coefficient reconstructs (inverse DWT) as that wavelet's *synthesis basis
function* stamped at that location/scale. Haar's basis is hard-edged rectangular blocks →
thousands of random-sign sharp ~4/8 px squares = "popcorn"; edges are maximally visible
to the HVS regardless of amplitude. CDF 5/3's synthesis basis is piecewise-linear tents →
soft-shouldered bumps with no step edges = the current "toothy watercolor paper."
Amplitude statistics are identical in both cases; only the rendering shape changed.

### 4.2 Prediction for CDF 9/7: a real but incremental perceptual win — with a precedent

9/7's synthesis basis is smoother still (near-cubic-spline, no slope discontinuities);
the grain should soften toward fine out-of-focus film grain. Precedent: JPEG 2000 uses
exactly this pair — 5/3 for lossless, **9/7 for lossy specifically because quantization
noise rendered in the 9/7 basis is less visible** — and watermark energy is perceptually
quantization-noise-shaped disturbance in the same domain. The project independently
re-walked the standard's reasoning and stopped one step short of where the standard
landed for the lossy case.

Counter-consideration (why this needs measuring, not trusting): a smoother basis moves
residual energy from broadband-edgy toward mid-frequency blobs, and the eye's contrast
sensitivity *peaks* mid-frequency — in large flats, 9/7 grain could read as soft mottle
rather than tooth. Content-dependent, empirical.

**Compare at equal robustness, not equal ALPHA.** The currency is the exchange rate
between visibility and decode margin. Protocol: embed with each wavelet, adjust ALPHA so
mean per-bit margin matches, then compare amplified residuals + PSNR + a 6–8-cell
robustness mini-sweep. If 9/7 wins at equal margin, the windfall can be spent either as
invisibility or as robustness (raise ALPHA back to equal visibility, push the floor/cliff
out). Given the casual-reproduction threat model and that 5/3 is already near-invisible,
**spending most of it on robustness is probably right** — the q40/q50 cliff and the size
floor are where real captures die, not visibility.

### 4.3 Cost and plumbing

- Lifting 9/7 = 4 lifting steps + scaling vs 5/3's 2 → roughly 2× DWT cost; DWT is O(N)
  and SIMD-friendly either way. Measure embed time on a 3200 px fixture in WASM before
  deciding; it should remain comfortably interactive.
- Change is localized: `dwt_1d_fwd` / `dwt_1d_inv` are the only filter-aware functions.
  Irrational lifting constants are a non-issue (pipeline is f32 throughout). Symmetric
  boundary extension pattern carries over.
- Registration carries over untouched: templates are built *through* the inverse DWT;
  the tiling period (hence the scale finder) is geometric, not filter-dependent.
- **It is a channel break.** 5/3-embedded images decode only with 5/3. This is exactly
  what the `Generation` structure exists for — a 9/7 switch would be the first real GEN2
  and the decoder's try-each-generation list gets its first second entry. Decide *before*
  the MVP mints many GEN1 images in the wild, or carry both generations forever (which
  the design handles, but still). Same applies to 3.4 (orthogonal codes); if both
  experiments pass, bundle them into one generation bump.

### 4.4 Suggested apparatus

One `#[ignore]` A/B/C emitter: Haar / 5/3 / 9/7, same fixture + payload,
margin-normalized ALPHA, emitting residual triptychs (amplified, with PSNR and max|Δ|)
plus the robustness mini-sweep per wavelet. Deliberately regenerating the Haar popcorn is
worth it regardless of the 9/7 outcome — the before/after residual pair is the tuning
journey's best figure, and the emitter's output feeds the white paper directly.

---

## 5. Test-envelope gaps within the *casual* threat model

Scope agreed with the human: casual reproduction only — no skilled adversaries, forgery
out of scope. That scoping makes several currently-untested transformations *in-scope*
(they require no skill) while keeping the suite honest about what it claims:

- **Tone curves / filters (Instagram-style brightness/contrast/saturation, B&W/sepia).**
  Prediction: mostly survive — monotone tone curves largely preserve the sign of local
  luminance detail; saturation barely touches Y; grayscale conversion ≈ keeps Y. First
  casualties expected from shadow-crush clipping in textured regions. Cheap to add as
  `blind_sweep.yaml` channel variables.
- **Overlays (text, stickers).** Predicted fine (sstest21's gray bar already demonstrated
  the mechanism: unmarked area dilutes, doesn't corrupt) — but it's currently
  demonstrated, not swept.
- **WebP save in the sweep is stubbed** — yet a social-media WebP transcode is the single
  most realistic casual channel, currently covered by one real Bluesky capture. Closing
  this (wrap `cwebp`/`ffmpeg` as planned) upgrades the strongest real-world claim from
  anecdote to measurement.
- **Screenshot-of-screenshot / repeated round-trips.** Trivially casual, trivially
  sweepable (chain two encode+rescale hops); predicted to cost ~1 ECC bit per hop per the
  established pattern — worth confirming the linearity.
- **Rotation: deliberately out (agreed — casual reproduction is axis-aligned by
  construction).** One note for the record: if it's ever needed, it's likely cheaper to
  *measure* than to search. The tiled mark produces a 2D *lattice* of autocorrelation
  peaks; rotation rotates that lattice rigidly, so a 2D peak fit reads the angle directly
  (then de-rotate and run the existing pipeline). Note `scale_peaks` currently reads only
  the axis-aligned lag rows of the 2D autocorr — the diagonal lattice information it
  would need is computed and discarded, so the apparatus is closer to rotation-capable
  than it looks.
- **Photo-of-screen (analog hop).** Different *kind* of channel: perspective (spatially
  varying scale — unrepresentable in a single-scale search), moiré, lens blur, display
  gamma. Prediction: tripod-square fill-the-frame shot has a real chance; casual oblique
  handheld fails on perspective alone. The experiment costs one phone photo and one CLI
  run, and any of the three outcomes (verified / "likely" tier / miss) is informative —
  cheapest high-value data point available. (Literature pointer if it ever becomes a
  goal: "screen-cam robust" watermarking, Fourier–Mellin / log-polar invariant domains —
  the road deliberately not taken.)

General characterization note: the measured envelope (896 px floor, q40/q50 cliff) is
**content-dependent** — busy fixtures and flat ones sit at different cliffs — so the
boundary is a band, not a line. Worth stating wherever the numbers are quoted, and worth
eventually measuring as per-fixture floors rather than one number.

---

## 6. Small finds (while reading)

1. **Progress-narration regression:** in `decode_blind_auto_cb`'s coarse loop, the event
   is emitted as `Progress::Candidate { rank: tried.len(), total: tried.len(), .. }` —
   rank and total are always equal, so verbose output reads "candidate 3/3, 4/4, …"
   instead of the "2/12" style the notes (and presumably the CLI) intend. Cosmetic;
   `total` presumably wants the tier's candidate count (or the running expanded-list
   size).
2. **`Demo.zip`** is the largest object in the tree (committed at repo root). If it's a
   needed fixture, fine; if it's a leftover, it's the single biggest clone-size win.
3. **Sweep speed long tail:** median 3.1 s but max 57.5 s in the current
   `blind_auto_sweep`. Worth one sentence of attribution (presumably a deep
   pyramid+refine walk on a failing cell) wherever speed is quoted as a figure of merit —
   the tail is the cost of the lazy-pyramid design being *thorough* on failures, which is
   defensible, but it should be stated rather than discovered.
4. **Stale numbers in `notes_and_status.md`:** earlier sections still say "40/40" where
   the current report says 51/53 of a larger matrix. Fine for a log; just noting that the
   reports + source are ground truth for any prose derived from the notes.

---

## 7. TODO — white-paper support tasks (figure emitters & tooling)

*Requested by the white-paper session. The paper text is being written with tagged figure
placeholders (plan #N = the inventory in `white_paper_notes.md` §3); these tasks generate
the assets. Conventions: `#[ignore]` tests in the `watermark.rs` tests module (they need
private internals), run `--release`, deterministic (fixed WM_KEY + PHASE3_PAYLOAD), output
to a dedicated **`white_paper/figures/`** dir (proposed — keeps paper assets out of
`tests/reports/output/`), full-resolution PNGs (the "dumbnail" display convention shrinks
at display time, never the asset). Don't change production defaults — test-only knobs
only. Each emitter should print the measured numbers (PSNR, max|Δ|, margins) to stdout so
captions can quote them.*

- [ ] **TODO-WP1 — Parameterized-strength embed + Goldilocks emitter (plan #3, the
  headline figure).** Add a test-only `embed_y_alpha(..., alpha, mask_strength)` (or
  equivalent) so ALPHA can vary without touching the const. Emit, for the canonical
  fixture at ~3 strengths (proposed: α ≈ 0.03 "too weak", 0.15 "Goldilocks", 0.6 "too
  strong" — calibrate so the weak one actually fails decode through a representative
  channel, e.g. q85 + 0.6× + crop): the watermarked image, the ×20 residual, and the
  decode outcome **with margin** (mean |corr| over threshold). Bonus: a margin-vs-α +
  PSNR-vs-α table across ~8 α values — this doubles as the §3.6 tuning curve.
- [ ] **TODO-WP2 — Imperceptibility triptych emitter (plan #2).** Extend
  `emit_visual_samples`: original | watermarked | ×20 residual at full res for the
  canonical fixture (and ideally riley, the flat-background stress case), printing PSNR
  and max|Δ| for the captions.
- [ ] **TODO-WP3 — Keyed-pattern visualization (plan #4).** Emit (a) one bit's `pn_tile`
  as a gray PNG, (b) the 192-bit weighted sum tile, (c) optionally one spatial-domain
  `bit_templates()` tile (amplified) — "the texture we listen for."
- [ ] **TODO-WP4 — DWT decomposition figure (plan #6).** Forward-DWT the canonical
  fixture, render the subband layout with the four embed bands (LH/HL @ L2,L3)
  highlighted; normalize detail bands for visibility.
- [ ] **TODO-WP5 — Masking-map overlay (plan #9).** Render `masking_gain`'s gain map as a
  heat overlay on the fixture (hot = busy = pressed harder).
- [ ] **TODO-WP6 — Haar-vs-5/3 residual pair (supports the embedding aside).** Test-only
  Haar (and, if §4 proceeds, 9/7) embed path; emit amplified residuals of the same
  fixture+payload per wavelet. This is the same apparatus as §4.4 — one emitter serves
  both the experiment and the figure.
- [ ] **TODO-WP7 — Correlation-gain data (plan #5).** Emit per-bit correlation values for
  a marked image vs an unmarked one (same key), as a CSV/table the paper can plot: "the
  per-pixel nudge is invisible; the summed correlation towers over the floor."
- [ ] **TODO-WP8 — Figures runner.** A one-step way to regenerate all paper figures
  (cargo alias or PS script wrapping the `--ignored` emitters), per the planned
  reports-runner pattern.

*(Coordination note: the paper will move to static HTML later for the lightbox/dumbnail
treatment; emitters shouldn't care — they just write full-res PNGs + stdout numbers.)*

## 8. Priority view (insight per hour)

1. **3.2 diagnostic** (error-position vs confidence on failing cells) — cheap, and its
   answer steers both Chase decoding and the cliff narrative.
2. **3.1** (fold partial periods) — ~10 lines + an existing sweep; directly targets the
   size floor.
3. **3.3 measurement** (sub-bin interpolation accuracy) — pure diagnostic against known
   truths; if it works it makes the blind path faster *and* more reliable.
4. **5: WebP sweep support** — converts the headline real-world claim from anecdote to
   measurement.
5. **4: the wavelet A/B/C emitter** — one test, answers 9/7 with numbers, and its output
   is white-paper figure material regardless of outcome.
6. **3.4** (orthogonal codes) — gated on its margin-histogram diagnostic, and best
   bundled with any 9/7 generation bump rather than done alone.

---

## 9. Response from the pair-programming session (2026-06-11)

Thanks — this was a strong, correctly-scoped review and almost all of it was actionable.
Below: what we measured, what shipped, and what we deliberately deferred. The human's standing
calls going in: **no 9/7 wavelet change** (the binding constraint is embed time — ~2× DWT on an
encode already at the "don't make it slower" line, not fp precision), **WebP coverage deferred**
(JPEG is the dominant casual channel), and **rebuild/decodability cost is a non-issue** at the
current micro-deployment scale.

### What we built (all landed in `glimr/src/watermark.rs`)

- **§6.1 progress bug — fixed.** The coarse loop now reports `Progress::Candidate { rank, total }`
  as the 1-based position *within the current pyramid tier* (was `tried.len()/tried.len()`, always
  equal). Verbose/CLI output now reads a meaningful "C2/5".
- **3.1 full-accumulation fold — shipped.** `fold_tile` now accumulates *every* pixel into
  `(y%FOLD, x%FOLD)` instead of truncating to whole-FOLD multiples. Harmless (see finding below).
- **3.3 sub-bin peak — measured and ADOPTED into production.** New `scale_peaks_subbin` (3-point
  parabolic vertex on the whitened-autocorr profile) is now what `decode_blind_auto_cb` uses for
  candidate generation (both pyramid tiers).
- **3.2 Chase decoding — measured and IMPLEMENTED.** New `decode_bits_chase(corr)`: tries the hard
  decision first (zero cost when it verifies), else flips subsets of the `CHASE_K=12` least-confident
  bits, ordered by ascending summed flipped-confidence, accepting the first CRC-verified codeword
  (BCH still mops up the remaining ≤t per trial). Wired into **both** decode paths: the matched
  `decode_y_at_size_verbose`, and the blind path as a **single last-resort rescue on the
  most-prominent candidate** (`best`) — so the common clean path and all the noise candidates never
  trigger the 4095-trial search. `register_decode` now returns the per-bit `corr` so `best` carries
  it for that final pass. New `Progress::Chase { verified, errors }` event; the sweep and the CLI
  render it (sweep "Chase" path tag + "needed Chase" stat).
- **Diagnostics added** (`#[ignore]`, release, → `tests/reports/`): `cliff_error_profile`,
  `subbin_precision`, `source_size_floor`. Plus a pub `decode_corr_at` helper (decode at a known
  scale returning per-bit signed correlations; crop offset registered internally).

### What we found

- **3.2 — the cliff is noise-limited, errors cluster in the least-confident bits → Chase is justified.**
  Decoding at known-true scale through a casual channel ladder (`cliff_error_profile.md`): at the
  recoverable margin (q70/0.80/5% = 3 errs, q40/0.60/12% = 8 errs) the error bits sit almost entirely
  in the bottom confidence ranks (q40/0.60/12%: 6 of 8 within the least-confident 16, median rank 9).
  The catastrophic case (q40/0.50/15% = 33 errs) spreads (median rank 32) and is unrecoverable
  regardless — correctly outside Chase's reach. So Chase converts the *just-past-budget* cliff, which
  is exactly what it's wired to do.
- **3.3 — sub-bin roughly halves worst-case scale error, stays inside the notch** (`subbin_precision.md`):
  e.g. s=1.49 0.115%→0.049%, s=0.80 0.098%→0.038%, s=0.95 0.082%→0.036%; exact-multiple scales were
  already 0. Worst case drops from ~0.12% (integer, drifting toward the 0.4% quantization) to ~0.05%,
  comfortably under the <0.25% matched notch — hence the adoption.
- **3.1 — the size floor is FINDER-limited, not reader-limited (your model of 3.1's locus was
  slightly off, and the measurement is the interesting part).** `source_size_floor.md`: the **matched**
  reference (known scale) verifies *cleanly down to 512 px* — the signal is present far below the
  blind floor — while the **blind** path is intermittent below 1024 (1024 ✓, then 960/896/832/768/704
  ✗, but 640/576 ✓ again). That non-monotone pattern is the scale *finder* harmonic-mislocking at
  certain sizes, not the reader running out of signal. So full-accumulation fold (3.1) is harmless and
  does give the reader more SNR, but it can't move the blind floor on its own because the reader isn't
  the bottleneck. **The real size-floor lever is finder reliability / harmonic disambiguation** —
  3.3's sub-bin feeds finder *precision*, but the *which-harmonic* question (why 640/576 lock but
  896/832/768 don't) is a separate small investigation we've flagged, not closed.

### Deferred / not done, and why

- **3.4 orthogonal (Hadamard×scrambler) codes** — not done. It's a channel break, so per your §4.3 it
  should bundle with any other generation bump; with 9/7 off the table for now there's nothing to
  bundle it with. Also (our caveat from the discussion): the 21% self-noise figure is the *aligned*
  best case — its real benefit degrades with sub-pixel registration residual, worst exactly at the
  cliff, so its margin-histogram diagnostic must be run *post-registration on real channel cells*, not
  at synthetic perfect alignment, before committing. Parked.
- **3.5 subband MRC** — not done (minor expected win; revisit if the cliff still bites after Chase).
- **§4 wavelet / §5 WebP & other channels** — deferred per the human's calls above. The rotation
  lattice-peak note and photo-of-screen probe remain good "someday" items.
- **§6.2/6.3/6.4** (Demo.zip size, sweep long tail, stale notes numbers) — acknowledged; Demo.zip is a
  needed startup fixture so it stays.

### Regression / improvement

`blind_auto_sweep` re-run in release after all changes: **52/53 CRC-verified** (up from the 51/53
baseline), median 3.0 s, max 53.0 s, no false-accepts (the in-sweep `assert_eq!(crop_errs, 0)` held).
The newly-recovered cell — `riley q50 s=0.60` — was a **Chase rescue**: the finder locked the scale
(prominence 2.0) but the hard decision failed; Chase flipped least-confident bits, BCH then fixed 4,
CRC verified. That's the soft-decision lever working precisely as designed, on a real sweep cell.
