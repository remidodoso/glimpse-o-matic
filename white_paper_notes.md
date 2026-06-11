# White Paper — Handoff Notes (agent → agent)

You are taking over authorship of `white_paper.md`. This file is your briefing: mission, audience,
voice, structure, the **visual/figure plan** (the human stressed this hardest), how to *generate*
figures from the codebase, and a technical source-of-truth digest so you don't have to re-derive the
facts (or get the math wrong). Verify specifics against the cited source files before asserting them —
the codebase is ground truth; these notes are a faithful but point-in-time digest (as of 2026-06-10).

This is **not** the white paper. Don't write in this file's voice in the paper. Don't commit anything
(the human handles git). Coding/perf work is being kept in a *separate* session; your job is the paper.

---

## 1. Mission & context

- **Glimpse-o-Matic** is a photo-gallery web app whose images are watermarked **at display time, in the
  browser** with an invisible, robust forensic mark. Purpose: if a viewed image is screenshotted and
  reappears elsewhere, the recovered mark identifies the source (and, eventually, the specific viewer).
- The paper is for an **MVP presentation to friends, family, and other interested people** — plus it's a
  forcing function for the human to understand the system clearly and explain it. So: genuinely
  explanatory, honest, not a sales deck.
- Outline already agreed and committed in **`white_paper.md`** (headers only). Treat that as the
  section structure; you may refine/reorder with the human, but don't silently restructure.

## 2. Audience & voice (locked in with the human)

- Register: **"science writer for an educated audience that reads engineering but doesn't necessarily
  know the math"** — think good long-form technical journalism. Target reader ≈ **1–2 years of college
  engineering/math**: comfortable with frequencies/Fourier intuition, noise/probability, logs/exponents,
  and reading a *clean* formula — **not** finite fields, wavelet theory, or estimator math.
- **Method:** intuition first (analogy → mechanism), then a formula **only where it sharpens** the idea,
  with the deep machinery in optional asides. **Layered / inverted-pyramid:** each section opens at a
  level a curious non-engineer can follow, then deepens — one document serving both audiences.
- **Voice:** active, first-person-plural ("we embed…"); concrete; candid about limits (the Limitations
  section is a feature, not an apology); willing to **narrate** (the blind-decode/scale-search
  investigation reads as a short detective story — use it). No marketing gloss.
- **Calibration sample** the human approved as the dial setting (match this level):
  > *The mark isn't a logo stamped on the picture. It's a faint, structured texture spread across
  > thousands of pixels, each nudged so slightly the eye can't catch it. Reading it back is like picking
  > a quiet, steady tone out of a noisy room: because we know exactly which pattern to listen for, we can
  > sum its echo across the whole image until it rises above the visual "noise" of the photograph itself.
  > Crop the image, shrink it, re-save it as a lossy JPEG — the pattern is repeated and redundant enough
  > to survive, and a 32-bit checksum tells us, with near-certainty, whether what came back is the real
  > message or just noise.*
- **Per-topic depth policy:** spread-spectrum & masking → pure intuition; DWT → "multi-scale split into
  coarse + detail bands, hide in chosen detail bands," no derivation; autocorr + whitening (scale
  finder) → a frequency picture + one relation (`bin ≈ N/period`); CRC → "a checksum verdict"; BCH/ECC →
  the *idea* of planned redundancy + why the budget is 4 errors, **Galois-field algebra goes in an aside
  or is named-and-cited, not derived.**

### Open decisions still to confirm with the human (he was asked, hadn't answered)
1. **Occasional set-off equations** (recommended) vs strictly prose+figures.
2. **"For the curious" sidebars/boxes** for the deep bits (BCH field math, exact DWT) — in or out.
3. **Figure placeholders now, real images generated later** — recommended yes (see §3–4).
Default to "yes" on all three unless he says otherwise; they're low-risk and fit the register.

## 3. THE VISUAL PLAN (the human's top priority)

**Mandate from the human, verbatim intent:** the 128×128 / 256×256 toy images of academic image-processing
papers are **wholly inadequate** here. Figures must use the **real fixtures** (2400–3200 px on the long
edge) at **meaningful resolution**, shown large, to demonstrate the actual perceptual and algorithmic
phenomena — especially **"not enough / too much / Goldilocks"** embedding strength. Use the **"dumbnail"**
convention already planned for the reports→journals work: commit full-res images, *display* them shrunk
(~320 px) with click-to-full; never downsample the asset itself (GitHub tolerates 100–200 MB).

**Residual-visualization technique** (state it in captions): the watermark is invisible by design, so to
*show* it, render the difference (watermarked − original) amplified — e.g. ×10–20 around mid-gray. Say so
in the caption so readers know the mark itself is far subtler than the figure.

### Figure inventory to produce (each: what it shows / source / how to generate)
1. **Pipeline schematic** — embed-in-RGB → display/zoom → screenshot (scale/crop) → re-encode → blind
   decode → payload + CRC verdict. *Hand-authored diagram (SVG/draw); not generated.* The spine of the
   paper.
2. **Imperceptibility triptych** — original | watermarked | amplified residual, with PSNR and max|Δ|.
   Real fixture, large. *Generatable; the `emit_visual_samples` `#[ignore]` test already emits residual
   PNGs — extend it.*
3. **Goldilocks strength sweep (the headline visual the human asked for)** — same image at **too weak**
   (mark below recoverability → decode fails), **Goldilocks** (`ALPHA≈0.15` + activity masking → invisible
   *and* robust), **too strong** (visible "orange-peel"/grain, worst in low-to-mid-luminance flats). For
   each column show *both* the picture (perceptual cost) *and* the decode outcome (robustness). Note: the
   embed strength is the `ALPHA` const (and masking); generating the sweep needs a **parameterized embed**
   (vary ALPHA) — today `embed_y_masked` exposes `mask_strength`, not `alpha`; add a small test-only
   parameterized embed or temporarily vary the const. *This figure is the core teaching device — budget
   real effort here.*
4. **The keyed pattern** — visualize a per-bit PN tile (the "structure" we listen for), amplified. From
   `pn_tile`/`bit_templates` in `watermark.rs`.
5. **Correlation gain** — a plot: per-pixel nudge is tiny, but the matched-filter response spikes far
   above the noise floor. *Generatable from the decode prominence values.*
6. **DWT detail bands** — an image decomposed; highlight the LH/HL bands at the embed levels where the
   mark lives. *Generatable from the forward DWT.*
7. **The scale-finder frequency story (great figure + narrative)** — the watermark's spectral peak moves
   with scale: **buried near DC when upscaled, clean mid-band when downscaled.** Back it with the real
   autocorr lag-profile data we measured: on sstest51 the true tile-period peak ranked **~#32** at full
   resolution but **#1 and exact at ½×**. Illustrate the pyramid (full→½→¼) relocating it. *We had a
   `probe_whitening` diagnostic that produced exactly these numbers (since removed — re-derive or
   re-add a generator).*
8. **Robustness matrix / channel waterfall** — decode success & residual-error vs JPEG quality, scale,
   crop — as a hand-colored heatmap (no plotting dependency). *Source: the `blind_auto_sweep` report
   (`tests/reports/blind_auto_sweep.md`) and `channel_waterfall`.*
9. **Perceptual masking map** — where the mark is stronger (busy/textured) vs weaker (flat), as a
   heatmap overlay. *Generatable from the activity mask in `embed_y_masked`.*
10. **Real-capture wins** — actual leaked-style screenshots (the `sstest*` captures in
    `tests/failed_crops/`) decoded: "this 1.49× zoomed, re-saved screenshot still yields the payload"
    (sstest51), plus the real Bluesky-laundered WebP. Powerful because it's *real*, not synthetic.
11. **Social-preview card** — an example composited Open Graph card from the `tools/social_preview` maker.

## 4. How to generate figures (mechanism)

- **Determinism is the enabler:** the watermark key (`WM_KEY`) and the test payload (`PHASE3_PAYLOAD`) are
  fixed, and embed/decode are deterministic. So generated figures are reproducible and **committable**
  without git-history bloat — the human explicitly wants generated images committed.
- **Existing apparatus (in `glimr/src/watermark.rs` tests module, all `#[ignore]`, run release):**
  `emit_visual_samples`, `channel_waterfall`, `scale_precision`, `sync_mechanism`, `crop_tolerance`,
  `blind_auto_sweep`. They read `tests/fixtures/*.jpg` (+ `captions.yaml`, `canonical` tag) and write to
  `tests/reports/` and `tests/reports/output/`. Pattern to follow: add `#[ignore]` "figure emitter"
  tests that write paper figures to a dedicated dir (propose `white_paper/figures/` or
  `tests/reports/output/`). Run with e.g. `cargo test -p glimr --features registration --release
  <name> -- --ignored --nocapture`. **Always `--release`** (debug is ~10× slower; this is a standing rule).
- **Reports→journals plan** (in `notes_and_status.md`, "Test & Demonstration Apparatus" + "Planned —
  reports → public journals"): the intended path is `.md` (GitHub-browsable) + standalone `.html`
  (CSS-overlay lightbox) from one run, dumbnail display, lazy-load. The white-paper figures can ride this
  same generation pipeline. Coordinate: figures probably want to live with the paper, but reuse the
  emitter pattern.
- For figures needing parameters not currently exposed (e.g. the ALPHA strength sweep), add **test-only**
  helpers; don't change production defaults.

## 5. Technical source-of-truth digest (so the prose is accurate)

Verify against source (file pointers in §6). Numbers as of 2026-06-10.

**Watermark embedding** — spread-spectrum in the **luminance (Y)** channel via a **CDF 5/3 wavelet (DWT)**.
A keyed **64×64 PN tile** (`TILE_SIDE=64`) is modulo-tiled across the **LH/HL detail subbands** at
`EMBED_LEVELS = [2, 3]`. Global strength `ALPHA = 0.15`; **perceptual masking** is **activity-based**
(`MASK_STRENGTH = 0.5`) — more mark where texture hides it, less in flat areas. (A *luminance*-based
masking attempt — "Phase 6" — backfired with visible orange-peel in low/mid flats and was **reverted**;
mention as a design lesson if useful.) Embed runs in RGB at display time; only the **watermarked** pixels
are ever exported (download = JPEG q0.92 of the watermarked Y-delta); the un-watermarked original never
leaves as a file.

**Payload** — **192 bits = 128 data + 32 CRC-32 + 32 BCH parity.** ECC is **BCH(192,160), t=4** over
GF(2⁸) (primitive poly `0x11D`), shortened from BCH(255,223); Berlekamp–Massey + Chien search, binary
(no Forney). Decode pipeline: **CRC-first → ECC-on-failure → CRC-recheck.** CRC is the definitive verdict.
There's a `Generation`/`GEN1` payload descriptor so the payload format can evolve (see §Future).

**Blind recovery (registration; native-only, feature `registration`, uses rustfft)** — recovers
unknown **scale** and **crop offset** then reads the bits, with no original. Two detectors:
- **Scale finder** = *unkeyed* spectrally-whitened **autocorrelation** of a centre block (`SCALE_BLOCK`,
  was 1024). The tiled mark shows as periodic spectral peaks; whitening (box-blur-of-power envelope,
  radius ~6) flattens the spectrum so the peak stands out. Period↔scale via `SCALE_REF=256`
  (`scale ≈ lag/256`). **Fold** size `FOLD=512` (LCM of the level-2 256 and level-3 512 tile periods).
- **Reader** = *keyed* matched filter (`register_decode`): correlates the folded tile against the known
  per-bit templates → reads sign bits → CRC/ECC. Keyed ⇒ far more sensitive than the unkeyed autocorr.

**The scale-search investigation (prime "detective story" material; landed 2026-06-09–10):**
- Symptom: *upscaled* (zoomed-screenshot) crops failed; *downscaled* crops decoded fine.
- Measurement (matched-filter sweep, bypassing the finder) proved the mark **survives the zoom** — every
  failing crop verified at its *true* scale (e.g. sstest51, 1591², verified at scale **1.49×**). So the
  failure was the *scale finder*, not signal loss and not the resampler.
- Root cause (the teachable insight): the unkeyed autocorr's peak sits at DFT **bin ≈ N/period**;
  **upscaling grows the period → pushes the peak toward DC**, where the image's own 1/f energy dominates
  and the whitening's box-blur (radius 6) gets inflated by nearby DC energy and divides the faint peak to
  nothing. A **whitening-profile experiment** confirmed it: at full res the true peak ranked **~#32**;
  with a smaller radius **~#6** but imprecise; at **½× it's #1 and exact under any whitening.** A bigger
  block does **not** help (more periods, same peak frequency). The keyed matched filter is immune
  (correlates the pattern, not spectral peaks).
- Fix: a **lazy multi-scale (pyramid) autocorr** — try full-res peaks first (fast path), then ½ then ¼
  only on failure; a level-`d` lag `L` maps to full-image scale `L/(d·256)`, so downscaled levels surface
  the *upscale* candidates the full-res finder is blind to; **decode at full resolution**. Bounded by
  `--max-size` (default 4000; drops tiny-scale candidates implying an implausibly large source — also the
  slow giant resamples) and `MIN_SOURCE=512` (drops scales implying a source too small to carry a mark;
  512 chosen so a strong-but-unreadable hit still reports "likely"). Refine pass is **prominence-ranked**.
- Floors (clean, canonical fixture): matched (scale-known) verifies down to ~**896 px** source (768 just
  fails); the *blind finder* fails earlier (~1024, mislocks the ½-harmonic). So the finder, not the
  signal, was the bottleneck.
- Results: sstest50/51/52 all recover (incl. the 1.49× upscale); `blind_auto_sweep` 51/53; the
  remaining failures are extreme `q40/q50` + heavy-crop — genuinely below the recovery floor even at the
  exact scale (good "cliff" material for Limitations).

**Robustness envelope (measured):** survives crop, scale ~0.5–1.5× (and beyond, via the pyramid), JPEG
~q80–q90 (realistic screenshot saves), and a **real Bluesky-laundered WebP** capture. Cliff: heavy
downscale + aggressive low-quality JPEG (q40/q50) overruns ECC. Robustness reported in `blind_auto_sweep`
with a secondary **speed** figure of merit (median decode ~3 s release; template build cached via OnceLock).

**Architecture** — thin **JS bootstrap + Rust/WASM (`glimr`)** doing image processing, the streaming zip
parser, and canvas rendering. **Streaming zip loader**: a hand-rolled state machine parses local file
headers as bytes arrive (stored + deflate), XOR-decodes `.dat` entries, decodes images progressively.
**Render pipeline**: three canvases (`#photo` visible, `#backing` off-screen compose target — *kept
intentionally to avoid prototype-era flicker, do not remove*, `decode` scratch) + a **screen-size scroll
cache** (per-image pre-scaled watermarked surface, built once, blitted per frame) that made the swipe
smooth (per-draw ~25 ms → ~12 ms). **Caching**: full-res watermarked RGBA in `pixel_cache` (250 MB LRU,
evict farthest-from-current); JIT/"mostly on-demand" model — watermark current ±1 neighbours during the
user's natural pauses; **no** whole-gallery background grind (must scale to 50–100 images). Scroll cache
is lifetime-aligned with `pixel_cache` and flushed on any pane-geometry change.

**Viewer UI** — thumbnail **carousel** (now with kinetic "throw"/momentum on pointer drag, tap-to-catch),
full-image **swipe** navigation with slide animation, **zoom** mode (1:1 pan, full-res), floating action
buttons, info/about overlays. Landscape vs portrait swaps the carousel axis.

**Packaging & obfuscation** — `packg` packs a folder of JPEGs into a **Stored zip** of hash-named `.dat`
entries, each **XOR'd** (key `0xDEADBEEF`). XOR is *obfuscation, not security* — it just stops casual
direct viewing; the real protection is the display-time watermark (the original is never handed out
un-watermarked). Reserved `social_preview.{jpg,png,txt}` ride **first** and **un-encoded**.

**Tools** (`tools/`): **packg** (pack + obfuscate), **deployg** (deploy viewer+archive to local dir or
Cloudflare **R2**, per-gallery prefix; injects OG/Twitter meta into a `<!--OG-->` sentinel block at
deploy time), **watermark-decode** (blind forensic CLI; `--max-size` flag; verdict tiers
verified/likely/not-detected), **social_preview** (browser canvas tool to compose the 1200×630 card),
and **packclient** (*planned, not built*: bake the watermark into client deliverables + a manifest mapping
payload→client so a leaked client copy is attributable). `build.ps1` builds WASM (→ `pkg/`) + the tools.

**Social preview (Open Graph)** — crawlers don't run JS, so OG/Twitter tags must be in the served HTML
at request time → **stamped at deploy** per gallery; the preview image is a **standalone** object (no
data URLs, crawlers can't unzip). Composed in the browser tool; exported as **real JPEG** (a PNG renamed
`.jpg` failed on Signal, which honors Content-Type + caps preview size — good cautionary detail).
Validated end-to-end on Bluesky and Signal. "**Security inversion**": a stolen preview advertises the
photographer, so re-publishing becomes desirable rather than a pure loss.

**Future infrastructure** — **payload evolution / generations** (the app & tools are written so the
payload can grow over time; `Generation`/`GEN1`). **Server-side identity** (Cloudflare **Workers + D1**):
reduce a viewer to a compact (~24–32-bit) key, embed a session/event ID, possibly a cryptographic
signature of the payload; keep an HTTP access log. To be approached **incrementally**, after the
static-hosting MVP is feature-complete.

**Limitations & non-goals** — **forensic deterrence, not DRM.** A determined adversary who knows the mark
is there can attack it (heavy blur, deliberate noise, re-shoot, crop below the size floor, extreme
recompression). Recovery holds only within the measured envelope (~896 px clean source floor; the
q40/q50 + heavy-crop cliff). The mark is luminance-domain and survives normal sharing/recompression but
is not a cryptographic guarantee.

## 6. Where to verify in the repo

- `notes_and_status.md` — the running design/status log; richest prose source. Key sections: Watermarking
  two-layer model; Obfuscation; Tools directory; ECC + Characterization findings; **Blind decode —
  upscale-crop recovery (2026-06-09)**; Test & Demonstration Apparatus + reports→journals plan; deployg;
  Social Preview (with the "implemented 2026-06-09" status block); Background Pre-Watermarking & cache.
- `glimr/src/watermark.rs` — embed (`embed_y_masked`, `pn_tile`, `bit_templates`), `bch` module, payload,
  `registration` mod (`scale_peaks`, `register_decode`, `decode_blind_auto[_cb]`, the pyramid + bounds),
  all the `#[ignore]` figure-capable tests, constants (`ALPHA`, `EMBED_LEVELS`, `MASK_STRENGTH`, `FOLD`,
  `SCALE_REF`, `TILE_SIDE`, `DEFAULT_MAX_SOURCE`, `MIN_SOURCE`, `PYRAMID_MIN_DIM`).
- `glimr/src/lib.rs` — WASM renderer: streaming zip state machine, three-canvas draw + scroll cache,
  `pixel_cache`/eviction, `is_reserved` (social-preview skip).
- `main.js` / `main.css` / `index.html` — viewer UI, carousel momentum, swipe/zoom, OG sentinel block.
- `tools/{packg,deployg,watermark-decode,social_preview}` — the tools (packclient not yet present).
- `tests/fixtures/` (canonical + captions.yaml), `tests/failed_crops/` (real `sstest*` captures),
  `tests/reports/` (+ `output/`) — figure source material and existing emitters.

## 7. Process constraints

- **Do not commit** — the human manages git.
- **Don't change production defaults** to make a figure; use test-only/parameterized helpers.
- Generated figures should be **deterministic and committable** (fixed key/payload).
- Keep `cargo test` defaults fast; figure emitters are `#[ignore]`, run `--release`.
- Confirm the three open voice/format decisions (§2) with the human before a full pass; safe defaults = yes.
- Suggested first deliverable once writing starts: the **Overview** + the **Goldilocks strength figure**
  (the human's emphasized visual) as a vertical slice that proves the voice and the figure pipeline before
  scaling to all sections.
