# Glimpse-o-Matic — Technical White Paper

## Overview

## Threat Model & Protection Philosophy

## The Watermark

Every image in a gallery is delivered to the viewer's browser, and at the moment it is
displayed, the browser stamps it — invisibly — with a small message. If a screenshot of
that view later turns up somewhere else, we want to read the stamp back off the stray
copy and learn where it came from.

That job description contains three requirements, and they work against each other:

1. **It should be invisible.** These are photographs, presented as the photographer
   intended. A mark that degrades the picture defeats the purpose of showing it.
2. **It should be durable.** A shared copy is rarely pristine: it gets screenshotted,
   cropped, shrunk to fit a chat window, re-saved as a JPEG, and run through whatever a
   social network's image pipeline does on upload. The mark should survive all of that —
   *casual* handling by people who aren't trying to remove anything.
3. **It should be readable blind.** When a suspect copy surfaces, we have only the copy.
   We don't know what it was cropped from, how much it was resized, or which original to
   compare it against. The mark should be recoverable from the stray image alone.

Each of the obvious approaches fails one of these. A visible logo is visible, and is also
the first thing a crop removes. Metadata (EXIF and friends) is stripped by the first
screenshot. A message hidden in the lowest bits of pixel values is invisible but is
erased by the first lossy save. What satisfies all three requirements is a technique
borrowed from radio engineering: **spread-spectrum signaling**.

The idea: rather than putting the message *somewhere* in the image — where it would be
either visible or easy to remove — we spread it *everywhere*, very faintly. The mark is a
structured texture laid across millions of pixels, each nudged by an amount too small to
see. Reading it back is a matter of knowing exactly which pattern to look for: the
decoder sums the pattern's faint echo across the whole image, and the total stands out
from the photograph's own visual noise even though no single pixel does. No pixel carries
any particular part of the message; all of them together carry it many times over. The
same redundancy is what survives cropping, shrinking, and recompression — most of the
signal can be lost, and what remains still adds up.

The rest of this section follows the mark through its life: how it is **embedded** in an
image, what the 192-bit **payload** contains and how it protects itself against read
errors, how a stray copy is **registered** — scale and crop offset recovered from the
mark's own structure — and read, and where the measured limits lie.

> **[FIGURE — pipeline schematic (plan #1).** The spine of the whole story: embed in the
> browser → display/zoom → screenshot (scale + crop) → re-save → blind decode → payload +
> CRC verdict. Hand-authored SVG; may live in the Overview instead — place wherever the
> reader first needs the map.]

### Embedding (Spread-Spectrum DWT)

**The mark lives in brightness.** A color image is, to the eye and to every image format,
a brightness picture (luminance) plus color overlaid (chrominance). We embed only in
luminance. Partly this is durability: image formats treat color as the expendable part —
JPEG normally stores it at half resolution — so a mark hidden in color would be degraded
first at every re-save, while brightness is preserved by every format, every screenshot,
and even a black-and-white conversion. And partly it is control: working in one channel
means the embedder makes one kind of change, whose visibility we can reason about and
tune.

**Choosing the hiding depth.** Within the brightness picture, *where* should the nudges
go? Our tool for answering this is the **discrete wavelet transform (DWT)** — for our
purposes, a way of splitting an image into layers by scale of detail. One pass separates
the image into a half-size coarse version plus the fine detail needed to rebuild it;
applied repeatedly, it yields a stack of detail layers — the finest texture, then
structure a few pixels across, then coarser features, and so on down to a small blurry
thumbnail at the bottom. The transform is exactly reversible: alter any layer, rebuild,
and the result is the original image plus an alteration at exactly that scale.

The layers make the hiding problem concrete, because the threats sort themselves by
scale. JPEG compression discards the *finest* detail first, so a mark hidden in the top
layer is erased by the first lossy save. The *coarse* layers are where the image's own
energy is concentrated, so changes there show up as visible blotches and shading shifts.
In between is a usable middle: detail at the scale of several pixels, fine enough that
the eye reads it as texture rather than as a feature, coarse enough that compression
preserves it. We embed in two adjacent mid-scale detail layers (levels 2 and 3 of the
decomposition — structures roughly four and eight pixels across), in both the
horizontal- and vertical-detail bands of each. Two layers rather than one is itself a
form of redundancy: resizing an image shifts energy between adjacent scales, and a mark
that spans two layers keeps a presence on whichever side the shift favors.

> **[FIGURE — DWT decomposition of a real fixture (plan #6).** The image split into its
> coarse core and detail bands, with the four embedding bands (LH/HL at levels 2 and 3)
> highlighted. Generate from `dwt_2d_fwd` on the canonical fixture.]

**The pattern is the key.** What we add to those layers is noise — but *our* noise. For
each of the 192 payload bits, a pseudo-random 64×64 grid of +1s and −1s is generated from
a secret key: statistically indistinguishable from coin flips, but exactly reproducible
by anyone holding the key, and by no one else. To embed a bit, its grid is added to the
detail coefficients as-is (for a 1) or sign-flipped (for a 0); all 192 grids are summed
and laid down on top of one another, everywhere. Each individual coefficient receives a
small, random-looking nudge — the sum of 192 coin flips. The message is not *in* any
coefficient; it is in the faint statistical lean of millions of them, and only the
keyholder knows which lean to measure. The same key that makes the mark recoverable is
what keeps it private: without the pattern there is nothing to correlate against, and
the mark is just a small amount of extra grain.

> **[FIGURE — the keyed pattern (plan #4).** One bit's PN tile, and the 192-bit weighted
> sum, rendered as amplified gray images: "this is the texture we listen for." Generate
> from `pn_tile` / the weighted tile in `embed_in_subband`.]

**Wallpaper, not a poster.** The 64×64 grid does not stretch to cover the band — it
*repeats* across it, like wallpaper, edge to edge. This is the crop insurance. A poster
cropped in half loses half its message; wallpaper cropped in half is smaller wallpaper.
Any patch of the image large enough to contain a few repeats contains, statistically, the
*entire* message, and the decoder later uses the repetition directly: it folds all the
repeats back onto a single tile, so every surviving copy of the pattern adds its evidence
to the same pile. Cropping costs only area — fewer repeats to average — never the message
itself. (The repetition also gives the mark a regular spatial period, which later becomes
the decoder's main clue for recovering an unknown scale; see *Blind Recovery & Scale
Search*.)

**How hard to press.** A single global strength constant (α = 0.15, in the transform's
coefficient units) sets how firmly every nudge is applied. It was tuned the obvious way:
downward from clearly visible until the mark disappeared into the photograph, then
checked against the decoder's ability to read it back through the capture chain. But a
single global pressure is wasteful, because images are not uniform. In busy texture —
foliage, fabric, hair — substantial nudges hide completely; in a flat sky, far smaller
ones show. So the embedder applies **perceptual masking**: it measures local detail
energy ("busyness") across the band and modulates the strength, pressing harder where the
image can hide it and easing off in the flats, while holding the *total* embedded energy
constant — the same signal power, spent where it costs the least visibility. The blend is
deliberately partial (a 50% lean toward the masked distribution rather than all the way),
a measured compromise: full masking concentrates so much energy at edges that it becomes
a different visible artifact, the ringing familiar from over-compressed JPEGs.

> **[FIGURE — Goldilocks strength sweep (plan #3) — the headline visual.** The same
> fixture at too-weak (invisible *and* unreadable — decode fails), α = 0.15 with masking
> (invisible and robust), and too-strong (visible grain in the flats). Each column shows
> the picture and the decode outcome, ideally with the decode margin, so the reader sees
> both sides of the trade at once. Needs the parameterized-ALPHA test embed — see
> feedback.md TODOs.]

> **[FIGURE — masking map (plan #9).** The activity gain as a heat overlay on the
> fixture: hot in texture, cool in flats — "where the mark presses harder." Generate from
> `masking_gain`.]

**Back to pixels, and the cost.** The modified layers are inverse-transformed, the new
brightness is written back under the untouched color, and the result is what the viewer
sees and what every export contains. The perceptual cost: a typical pixel moves by
roughly one gray level out of 255, and even the most-changed pixels move by only a few.
Amplified twentyfold, the difference between original and marked image shows up as a
fine, slightly organic grain — we describe it as "toothy watercolor paper." At actual
strength, we have not been able to find it by eye, even in side-by-side comparison,
knowing where to look. The texture's particular character is itself a design choice with
a lesson in it (see aside).

> **[FIGURE — imperceptibility triptych (plan #2).** Original | watermarked | residual
> ×20, full resolution, with measured PSNR and max-pixel-change in the caption — the
> caption must say the residual is amplified, or the figure lies in the wrong direction.
> Generate by extending `emit_visual_samples`.]

> **For the curious — the recipe in one line.** With the image's luminance
> wavelet-transformed, every coefficient at tile position *t* in an embedding band is
> updated as
>
> &nbsp;&nbsp;&nbsp;&nbsp;*coeff* += α · *g* · Σ_b ( *s_b* · *pn_b*[*t*] )
>
> where α = 0.15 is the global strength, *g* is the local masking gain (mean 1 — it
> redistributes energy without adding any), and the sum runs over all 192 payload bits:
> *s_b* = ±1 is bit *b*'s value and *pn_b* is its keyed ±1 tile. Decoding will be this in
> reverse — correlate the same *pn_b* against the coefficients and read each bit back
> from the *sign* of the result. Constants, for reference: 64×64 tiles, embedding in the
> LH/HL bands at decomposition levels 2 and 3, masking blend 0.5, CDF 5/3 wavelet.

> **Aside — why the grain looks the way it does.** The wavelet is the *font* the mark is
> printed in. Whatever we add to a detail layer is rendered into pixels as that wavelet's
> characteristic shape, stamped at every nudged position. Our first build used the
> simplest wavelet (Haar), whose shape is a hard-edged block — and the mark rendered as a
> faint field of sharp little squares, recognizably artificial "popcorn," because the eye
> picks out edges at amplitudes where it ignores almost everything else. Switching to a
> smoother wavelet (CDF 5/3, whose shape is a soft-shouldered tent — the same family
> JPEG 2000 uses) changed nothing about the signal's strength or statistics, only its
> rendering: the identical nudges now draw the soft papery grain described above. Same
> message, better penmanship. There is likely one more step on this ladder (a smoother
> wavelet still, 9/7 — the one JPEG 2000 uses for lossy compression), an experiment noted
> for future work.
>
> **[FIGURE — Haar "popcorn" vs CDF 5/3 grain (supports the aside).** Two amplified
> residuals of the same fixture, same payload, Haar vs 5/3 — the tuning journey's best
> before/after. Needs a test-only Haar embed path; see feedback.md TODOs.]

### Payload, CRC & Error Correction

### Blind Recovery & Scale Search

### Robustness & Limits

## Application Architecture

### JavaScript ↔ Rust / WASM Division

### Streaming Zip Loader

### Rendering Pipeline & Image Caching

## The Viewer UI

## Gallery Packaging & Obfuscation

## Build & Deploy Tooling

## Forensic Decode & Client Attribution

## Social Preview (Open Graph)

## Future Infrastructure

### Payload Evolution & Generations

### Server-Side Identity (Cloudflare Workers + D1)

### Access Logging & Analytics

## Limitations & Non-Goals
