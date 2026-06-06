# Watermarking — Project Notes

## Goal

Embed an invisible spread-spectrum watermark into decoded images immediately before
display. If a watermarked image is later recovered (screenshot, save-as, phone photo of
screen) the payload can be decoded without the original image to identify when and on
what browser it was viewed.

Security goal: "moderate inconvenience" — not forensic-grade, not defeat-proof, but
a meaningful deterrent that survives casual exfiltration attempts.

---

## Requirements

- **Blind detection** — payload recoverable without the original *image*. Note: the
  resize-robust decoder is **size-informed** — it needs the original *dimensions*
  (not the pixels), which the gallery operator has. Fully-blind only at native /
  power-of-2 scales. (See Phase 4 for why: DWT shift-variance.)
- **Imperceptible** — not visible at normal viewing conditions; image never viewed 1:1
  so embedding strength can be higher than classical JND analysis suggests
- **Resize-robust** — survives downscale to screen resolution (~1MP) from 3–6MP source.
  With the original dimensions supplied, survives **arbitrary** scale factors
  (33%–120% tested, 0 bit errors), not just the 0.3–0.6× target range.
- **Crop-robust** — edge cropping only (aspect-ratio adjustment); not concerned with
  tight zoom crop
- **JPEG-tolerant** — survives re-save at quality ≥ 80
- **In-memory only** — applied after decode, before canvas write; never written to disk

---

## Payload — 128 bits (16 bytes)

| Bytes | Field | Notes |
|-------|-------|-------|
| 0–3 | Unix timestamp (u32 LE) | Captured at `receive_pixels` call time |
| 4–7 | IPv4 address (u32 LE) | 0x00000000 if unavailable |
| 8–11 | Browser fingerprint hash (u32 LE) | FNV-1a of UA + screen + lang + tz + cores + RAM |
| 12–13 | Referrer hash (u16 LE) | FNV-1a of referrer hostname, 0 if absent |
| 14 | Flags (u8) | bit 0 = referrer present |
| 15 | Version (u8) | = 1 |

Payload is assembled in JS and passed into WASM at each `receive_pixels` call.
Timestamp and referrer are per-image-view; nonce/fingerprint are computed once at
page load and cached.

IP: pre-fetch from `api.ipify.org` at page load (deferred — wired in when serving
infrastructure is clearer). Zero for now.

---

## Algorithm

### Domain
2D Discrete Wavelet Transform (DWT). **Currently CDF 5/3 (LeGall)** via the lifting
scheme (Stage 3) — Haar was the prototype. Standard separable (row-then-column) in-place
transform with whole-sample symmetric boundary extension; Mallat quadrant subband
layout, ceil split (`lo_len(n) = (n+1)/2` low-pass coefficients). CDF 9/7 remains a
possible future upgrade for marginally smoother synthesis.

### Channel
Y (luminance) channel only. Extracted as f32, watermarked, delta written back equally
to R/G/B. Alpha channel untouched.

### Perceptual masking (Stage 2)
Per-coefficient embedding strength is scaled by a local-activity map so the grain
hides in content. For each embed level the gain is derived from co-located detail
energy (|LH|+|HL|+|HH|, 3×3-smoothed), mapped `gain = (activity/mean)^MASK_GAMMA`,
clamped to `[MASK_FLOOR, MASK_CEIL]`, renormalized to mean 1, then **blended toward
uniform by `MASK_STRENGTH`** (`gain = 1 + MASK_STRENGTH·(gain−1)`). Both endpoints are
mean-1, so any blend stays **energy-neutral**: total embedding energy — and therefore
detection strength — matches uniform embedding; only the *distribution* moves. At full
strength that's ≈0.3× in smooth regions / up to ≈3× in busy regions; `MASK_STRENGTH`
dials that back. The blend exists because full masking piles energy onto **edges**
(where |LH|/|HL|/|HH| spike), which reads like JPEG ringing — 0.5 keeps the smooth-area
win without the edge halo. Decoding is
unaffected — correlation is sign-based, so the positive per-region gain never flips a
bit and the decoder needs no knowledge of the mask. Global PSNR drops slightly and
peak |Δ| rises (energy concentrates in busy areas) even though perceived visibility
improves — PSNR is the wrong metric for this trade.

### Embedding subbands
LH2, HL2, LH3, HL3 — mid-frequency detail bands (`EMBED_LEVELS = [2, 3]`).

For a 3000×2000 source:
- LH2/HL2: 750×500 = 375,000 coefficients each
- LH3/HL3: 375×250 =  93,750 coefficients each

**Visibility tuning (Stage 1):** levels were moved finer from [3,4] to [2,3]. Coarser
levels (8×8/16×16 footprints) read as blocky "popcorn"; finer levels (4×4/8×8) read as
finer grain the eye tolerates far better, and hold ~4× more coefficients (energy spread
thinner). Trade-off: finer bands are more attenuated by downscaling, so heavy
(below-requirement) downscales like 3× are marginal — see Phase 4.

**Wavelet (Stage 3):** switched Haar → **CDF 5/3**. Haar's box synthesis paints each
modified coefficient as a flat ±step with hard block edges (the residual mosaic); CDF
5/3's smooth, overlapping synthesis paints a gentle ramp, so the grain is continuous,
not tiled. 5/3 also has two vanishing moments (kills constant *and* linear content in
the detail bands vs Haar's constant-only), so smooth gradients leave a near-zero detail
background — the watermark is simultaneously **less visible and more detectable**.
Measured on test_a at ALPHA=0.15: PSNR 36.5 → **45.5 dB**, max|Δ| 37 → **15 LSB**, while
resize/JPEG detection scores held or improved.

### Spreading
One PN sequence per payload bit (128 total), seeded with `WM_KEY XOR bit_index` using
XorShift64. Yields ±1 as f32, arranged as a `TILE_SIDE × TILE_SIDE` grid that is tiled
across each subband by **repetition (modulo)** — coefficient `(r, c)` takes PN cell
`(r mod TILE_SIDE, c mod TILE_SIDE)`. Per-coefficient PN keeps the spatial texture fine
and pseudo-random (avoiding the coarse visible lattice that a single grid *stretched*
over the subband produces), and the repeating tile gives crop robustness. Each bit is
embedded in all four target subbands independently.

Resize robustness does **not** rely on the tiling: the size-informed decoder resamples a
suspect back to the original embedding dimensions, so each subband regains its original
size and the modulo indices reproduce exactly (see Phase 4). A normalized/stretched
tiling was tried for an abandoned decode-at-arbitrary-size approach; it bought nothing
here and produced a ~39px "meat tenderizer" lattice, so it was reverted.

### Embedding rule
For bit `b` with value `v ∈ {0,1}`, sign `s = 2v - 1`:
```
coeff[i] += ALPHA * s * pn_b[i % TILE_SIZE * TILE_SIZE]
```

### Blind detection
Correlation of suspect subband with the known PN sequence. Image content correlates
to ~zero (law of large numbers over 90k+ coefficients). Sign of the correlation gives
the bit value. Scan multiple DWT levels of the suspect image — the correct level shows
a strong correlation peak.

### Tuneable constants (in `watermark.rs`)
- `WM_KEY: u64` — secret PRNG seed (hardcoded in WASM for prototype)
- `ALPHA: f32` — embedding strength, **currently 0.15** (PSNR ≈ 37.6 dB, max|Δ| ≈ 25 LSB
  on 6MP images; was 0.3 / 31.6 dB before Stage-1 visibility tuning)
- `EMBED_LEVELS: &[u32]` — **currently `[2, 3]`** (was `[3, 4]`; moved finer for visibility)
- `TILE_SIDE: usize` — PN grid resolution (each subband normalized to TILE_SIDE²), default 64
- `PAYLOAD_BITS: usize` — default 128
- `DECOMP_DEPTH: u32` — DWT levels, default 4
- `MASK_FLOOR / MASK_CEIL: f32` — perceptual-masking gain clamp, default 0.30 / 3.00
- `MASK_GAMMA: f32` — softening exponent on the activity ratio, default 0.50
- `MASK_STRENGTH: f32` — masking blend (0 = uniform, 1 = full), default 0.50

---

## Integration Points

### WASM (`glimr/src/`)
- **New file**: `watermark.rs` — `embed()`, DWT, PN generation, subband helpers
- **Modified**: `lib.rs` `receive_pixels` — add `payload: &[u8]` parameter, call
  `watermark::embed` before storing in `pixel_cache`

### JavaScript (`main.js`)
- **New functions**: `fnv32a(str)`, `browser_profile_hash()`, `referrer_hash()`,
  `build_payload()` — called at `init()` (fingerprint) and at each `receive_pixels`
  invocation (timestamp)
- **`receive_pixels` call site** — pass `build_payload()` as extra argument

### Decoder (`tools/watermark-decode/`)
- Standalone Rust CLI binary
- Reads image (JPEG/PNG), runs blind correlation detection, prints decoded payload
- Human-readable output: timestamp, IP, fingerprint hex, referrer flag
- Initially duplicates detection code from `watermark.rs`; factor into shared crate later

---

## Implementation Phases

### Phase 1 — DWT round-trip
Implement Haar 2D forward and inverse DWT in `watermark.rs`.
**Test**: apply forward + inverse to a test image, assert max pixel error < 1 LSB.
No watermark yet. Pass/fail is purely mathematical.
Can run as a `#[test]` in the glimr crate compiled to native target.

### Phase 2 — Embed + immediate decode (same process)
Add PN generation and embedding in LH3/HL3. Add blind correlation decoder in the same
module. Embed a known 128-bit payload, immediately decode from the modified subband.
**Test**: decoded bits match embedded bits exactly (no file I/O, no JPEG, ideal
conditions). Also compute and print PSNR of the watermarked vs. original Y channel.
This validates the algorithm before any integration work.

### Phase 3 — JPEG roundtrip ✓
**Results (test_a.jpg, 2500×2500, ALPHA=1.0):**
- q90: 0/128 errors, PSNR=21.1 dB
- q80: 0/128 errors, PSNR=21.1 dB
- q70: 0/128 errors, PSNR=21.1 dB (below requirement — informational)
- Residual saved: `tests/residual_wm.png`

**ALPHA note**: 21 dB PSNR is visible at 1:1. SNR margin is very wide, so ALPHA was
later reduced for imperceptibility — now **0.15** (PSNR ≈ 37.6 dB). At ALPHA=0.15,
levels [2,3]: q70/q80/q90 all still decode 0/128 errors. `emit_residual` is in
`watermark.rs` for visual tuning.

**Visual quality (Stage 1):** the `emit_visual_samples` test writes
`tests/sample_original.png`, `sample_watermarked.png`, and `sample_residual.png`
(lossless, so JPEG doesn't confound the eyeball comparison) at the current settings.
See the "Visibility tuning" note under *Embedding subbands* for the rationale (finer
levels + lower ALPHA). Further reduction is possible via perceptual masking (Stage 2)
or a smooth wavelet / CDF 9/7 (Stage 3) — see Deferred.

### Phase 4 — Resize robustness ✓
**Results (test_a.jpg, 2500×2500, ALPHA=0.15, levels [2,3], Lanczos3 resampler),
all via the size-informed decoder:**

| scale | suspect | errors | alignment score |
|-------|---------|--------|-----------------|
| 33% (1/3) | 833×833 | 3/128 | 18 — below requirement, informational |
| 50% (1/2) | 1250×1250 | 0/128 | 37 |
| 57% (4/7) | 1428×1428 | 0/128 | 39 |
| 60% (3/5) | 1500×1500 | 0/128 | 42 |
| 70% (7/10) | 1750×1750 | 0/128 | 48 |
| 120% (6/5) | 3000×3000 | 0/128 | 60 |

(Off-grid / no-watermark noise floor ≈ 6–15, so the score doubles as a
detection-confidence metric — see `detection_strong()` / `detection_floor()`.)

Stage-1 visibility tuning (lower ALPHA, finer levels) shrank the scores from the
~110 they were at ALPHA=0.3 / levels [3,4]. In-requirement scales (0.4–0.6×, i.e.
57–70%) plus 50% and 120% still decode cleanly; the 3× downscale (33% → 0.7 MP) is
below the stated requirement and no longer survives — recovering it would cost
visibility (higher ALPHA / coarser levels) or need ECC.

**The journey to this result is worth recording, because two earlier approaches
failed and the reason is fundamental.**

1. **Original subband-coordinate (modulo) tiling** — `pn[r % TILE_SIDE …]`.
   Worked only at exact power-of-2 scales (50% → 0 errors); every other factor
   collapsed to ~random because the tile period is tied to absolute coefficient
   spacing, which the resize changes.

2. **Normalized tiling + fractional-octave decode search (Strategy B)** —
   normalized the PN grid to fractional subband coordinates (kept — necessary),
   then had the decoder downscale the suspect by ρ over one octave and level-scan,
   hoping to snap the signal onto a clean dyadic level. **Failed**: non-power-of-2
   cases stayed at ~47/128 errors and the search peaked on the *wrong* ρ. The
   near-ideal ρ scored no better than noise.

   **Root cause: the critically-sampled Haar DWT is shift-variant.** A
   non-power-of-2 resample shifts image content relative to the dyadic block grid
   that the level-3/4 coefficients are computed on, scrambling those coefficients
   even though the frequency content survives. No amount of frequency-band
   realignment recovers a shift-scrambled critically-sampled transform.

3. **Size-informed decoder (`decode_y_at_size`) — the fix.** Since recovery
   requires the *exact original pixel grid*, the decoder resamples the suspect
   back to the original embedding dimensions, then runs the matched decoder
   (`EMBED_LEVELS` × {LH, HL}). With the grid restored, every scale factor decodes
   cleanly. (Normalized tiling was briefly used here but later reverted — modulo
   tiling reproduces exactly once the grid is restored, and looks far less
   patterned; see *Spreading*.)

**Blind recovery (the `--scan` mode).** The alignment peak is razor-sharp: at the
exact original size the score peaks; off by just 2% it collapses to the noise floor.
This *defeats hierarchical (coarse-to-fine) search* — a sharp peak has no shoulders to
climb, so a coarse pass steps over it seeing only noise. But it does **not** defeat
*exhaustive* search: the true original size is an integer, so testing **every integer
long-dimension size** (step 1 px) is guaranteed to hit it. The short dimension is
derived from the suspect's aspect ratio (±1 px for rounding), making it a 1-D sweep
(aspect-preserving rescale only — cropping breaks the regrid regardless). Candidates
are independent → run on a bounded `rayon` pool (`--threads`, default 4). A genuine hit
shows as a clear peak plus a small cluster of elevated scores at O±1–2 px — itself a
confidence signal; the runner-up candidates are printed so a human can sanity-check.

The cheaper path when available is still **semi-blind**: a gallery operator knows which
source image leaked, hence its exact dimensions (`--size`/`--ref`), or can try the
handful of known gallery sizes (which also identifies *which* image leaked). `--scan` is
the fallback when the original dimensions are genuinely unknown.

**Decoder API** (`watermark.rs`):
- `decode_y(y, w, h)` — matched decoder; exact at the original (native) resolution.
- `decode_y_at_size(y, w, h, orig_w, orig_h)` — size-informed, handles arbitrary scale.
- `decode_y_at_size_verbose(…)` — also returns the alignment score (confidence).
- `resample_y(…)` — dependency-free separable triangle-filter resampler (WASM-safe).

(The old level-scanning `decode_y_scan` was removed when switching to CDF 5/3 — its
all-levels correlation sum picked up too much cross-level noise from 5/3's larger
detail coefficients. Blind = native via `decode_y`; any rescale uses `decode_y_at_size`.)

### Phase 5 — WASM integration ✓
**Changes:**
- `lib.rs` `receive_pixels` now accepts `payload: &[u8]` (16 bytes from JS).
  Extracts Y channel → embeds watermark → writes delta back → caches watermarked pixels.
  Logs `"image N WxH watermarked in Xms"` for performance monitoring.
- `main.js` adds: `fnv32a(str)`, `g_browser_fp` (FNV-1a of UA+screen+lang+tz+cores+RAM,
  computed once at page load), `g_referrer_hash` (FNV-1a of referrer hostname & 0xFFFF),
  `build_payload()` (assembles 16-byte payload with per-call timestamp).
- `decode_image` call site updated: `receive_pixels(..., build_payload())`.
- WASM rebuilt; generated binding: `receive_pixels(i, width, height, data, payload)`.

**Performance**: open the browser and watch the console for `watermarked in Xms` lines
to characterise throughput.  Expected: ~100–500ms on desktop for 3–6MP images.

### Phase 6 — CLI decoder tool ✓
`tools/watermark-decode/` — workspace member, built by `build.ps1` to `tools/bin/`.
Accepts one or more JPEG/PNG paths via argv. The `source` line (dimensions + file
size) is printed the moment the image is read, before any (possibly long) decode:
```
path/to/image.jpg
  source    : 2500×2500  (6.25 MP, 7.5 MB)
  best fit  : 2500×2500  (resampled from 1750×1750 suspect)     # --scan only
  detection : almost certain  (score 69, 160σ above noise median 7; version field valid)
  version   : 1
  timestamp : 2026-06-05 14:23:11 UTC  (unix 1749133391)
  ...
  candidates:                                                   # --scan only
     2500×2499  score 59 (133σ)
     ...
```
Modes:
- `--size WxH` / `--ref <original>` — supply the original embedding dimensions; the
  suspect is resampled back to that grid (`decode_y_at_size`) and decodes at **any**
  scale factor.
- `--scan [MIN:MAX]` — brute-force every long-dimension size in `MIN..=MAX` (default
  1000:4000) when the original dimensions are unknown; threaded via `--threads N`
  (default 4). Surfaces a running best (`★` lines + progress %) live, prints the best
  fit plus runner-up candidates, and **Ctrl-C** stops the sweep and reports the best
  found so far (handler only sets an atomic flag; workers bail; printing stays in
  normal code).
- (no mode) — blind `decode_y` (via `decode_y_at_size_verbose` at native size), assumes
  the image is already at its original resolution.

**Confidence.** The `detection:` line gives a qualitative band backed by numbers:
- In `--scan`, the sweep collects its own noise reference — the ~thousands of wrong-size
  scores. The peak is reported as **σ above the noise floor** (robust median + MAD, so
  the signal cluster near the true size doesn't inflate it). Bands: `not detected` (<3σ)
  / `weak` (<6σ) / `likely` (<12σ) / `almost certain` (≥12σ).
- In `--size`/`--ref`/blind there's no distribution, so the band uses the ALPHA-derived
  `detection_strong/floor` thresholds.
- Both corroborate with a **structural self-check**: the payload's `version` byte must be
  1 (a weak built-in checksum). "version field valid/INVALID" is reported alongside the
  statistical verdict — the two signals are independent. A real CRC/ECC in the payload
  (Deferred, coming soon) will turn this into a rigorous "verified" statement; per-bit
  correlation margins ("128/128 bits strong") are a planned fast-follow.

**Tests** (`cargo test -p watermark-decode`):
- `decode_no_crash_on_unwatermarked_image` — smoke test, no panic
- `roundtrip_known_payload_direct` — embed → decode (no JPEG), exact match
- `roundtrip_known_payload_via_jpeg_q80` — full JPEG pipeline, 0 errors
- `cli_binary_decodes_watermarked_jpeg` — runs release binary, parses output
- `cli_decodes_resized_suspect_with_size_flag` — 70% downscale recovered via `--size`
- `cli_scan_recovers_unknown_size` — 70% downscale, size recovered by `--scan`

---

## Deferred

- **ECC / checksum** (Reed-Solomon / repetition code + CRC) — **coming soon**. A CRC in
  the payload turns the decoder's structural self-check (currently just `version == 1`)
  into a rigorous "verified" verdict; ECC adds error correction so a few flipped bits
  still recover. Also enables a per-bit-margin confidence readout ("128/128 bits strong").
- ~~**Perceptual masking**~~ — DONE (Stage 2): per-coefficient gain from local detail
  energy, mean-1 normalized. See *Perceptual masking* under Algorithm. Possible future
  refinement: a proper NVF/CSF model and per-level `MASK_*` tuning.
- **IP address** — wire in when serving infrastructure is clarified
- ~~**CDF 5/3 wavelet**~~ — DONE (Stage 3): replaced Haar; lifting + symmetric extension,
  ceil split. Big imperceptibility win (see *Wavelet (Stage 3)* under Algorithm).
- **CDF 9/7 wavelet** — optional further smoothness over 5/3 (4 lifting steps + scaling
  vs 5/3's 1+1). Marginal perceptual gain; revisit only if 5/3 grain is still objectionable.
  Note: 9/7 is also critically sampled, so it would *not* remove the shift-variance that
  forces the size-informed decoder; an **undecimated/stationary wavelet transform (SWT)**
  would, at the cost of redundancy/compute.
- ~~**Fully-blind scale recovery**~~ — DONE (`--scan`): brute-force every integer
  long-dimension size, threaded. Works because the sharp peak, while it defeats
  hierarchical search, is trivially hit by an exhaustive 1px sweep. A *computed*
  (non-brute-force) recovery would still need a shift-invariant domain
  (FFT-magnitude / Fourier–Mellin) — not worth it given `--scan` suffices offline.
- **Gallery auto-match** — given the alignment score peaks only at the correct
  original size, the decoder could try every known gallery dimension and report
  which source image a leak came from. Primitive (`decode_y_at_size_verbose`) is in place.
- **Web drag-and-drop decoder** — local HTML+WASM, key entered at runtime; after CLI works
- **Shared `watermark-lib` crate** — factor out when decoder and embedder both exist
- **Authentication / identity injection** — possibly Cloudflare Worker + Google OAuth later

---

## Status

- [x] Design chosen — DWT spread spectrum, 128-bit payload, blind correlation detection
- [x] Phase 1 — DWT round-trip
- [x] Phase 2 — Embed + immediate decode
- [x] Phase 3 — JPEG roundtrip
- [x] Phase 4 — Resize robustness
- [x] Phase 5 — WASM integration
- [x] Phase 6 — CLI decoder

---

*(General project milestones go in notes_and_status.md. This file tracks
watermarking-specific design and implementation details.)*
