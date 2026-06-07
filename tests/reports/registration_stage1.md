# Registration Stage 1 — does a 512×512 excerpt reveal the watermark period?

Source: `tests/test_a.jpg` (2500×2500).  CDF 5/3, ALPHA=0.15, levels [2, 3].
Level-2 tile period = 256 px at embed scale; in a B-px block at scale s there are
B/(period·s) periods (≥2 needed).  **prominence** = autocorrelation peak ÷ off-peak band
(≫1 = clear lattice; ~1 = invisible).  **oracle** = autocorrelation of the pure watermark
delta (content removed) = the ceiling.  Blind methods: **spectral** = spectral-whitened
autocorrelation (scale-agnostic), **dwt-band** = wavelet band-pass then autocorrelation.

Regenerate: `cargo test -p glimr --release registration_stage1 -- --ignored --nocapture`

## Slice 1 — block size × scale (centre block, masked embed @ 0.5)

| block | scale | period px | det px | scale err | spectral | dwt-band | oracle |
|-------|-------|-----------|--------|-----------|----------|----------|--------|
|  256 | 1.00 |  256.0 |  193.0 | -24.6% |    0.1 |    0.0 |    0.1 |
|  256 | 0.70 |  179.2 |  158.0 | -11.8% |    1.6 |    0.8 |    1.2 |
|  256 | 0.50 |  128.0 |  128.0 |  +0.0% |   17.4 |    2.1 |   10.9 |
|  256 | 0.33 |   84.5 |   84.0 |  -0.6% |   11.9 |    1.9 |   17.1 |
|  256 | 0.25 |   64.0 |   64.0 |  +0.0% |   15.3 |    3.7 |   32.0 |
|  512 | 1.00 |  256.0 |  256.0 |  +0.0% |   18.9 |    2.5 |   18.3 |
|  512 | 0.70 |  179.2 |  179.0 |  -0.1% |   51.2 |    4.0 |   41.3 |
|  512 | 0.50 |  128.0 |  128.0 |  +0.0% |   88.2 |    3.6 |   51.5 |
|  512 | 0.33 |   84.5 |   84.0 |  -0.6% |   22.6 |    3.5 |   25.6 |
|  512 | 0.25 |   64.0 |   64.0 |  +0.0% |   15.8 |    2.8 |   34.4 |
| 1024 | 1.00 |  256.0 |  256.0 |  +0.0% |  111.4 |    5.0 |   83.3 |
| 1024 | 0.70 |  179.2 |  179.0 |  -0.1% |  128.2 |    4.6 |   72.2 |
| 1024 | 0.50 |  128.0 |  128.0 |  +0.0% |   78.3 |    4.6 |   57.3 |
| 1024 | 0.33 | n/a (block > image) | | | | | |
| 1024 | 0.25 | n/a (block > image) | | | | | |

## Slice 2 — masking strength × block content (512 block, 0.5× scale)

| mask | block  | spectral | dwt-band | oracle |
|------|--------|----------|----------|--------|
|  0.0 | busy   |  109.5 |    2.5 |   53.8 |
|  0.0 | smooth |   35.8 |    1.7 |   53.3 |
|  0.5 | busy   |  102.4 |    2.5 |   53.9 |
|  0.5 | smooth |   48.5 |    1.8 |   49.5 |

## Slice 3 — whitening method × scale (512 block, centre) — prominence

| method     |   1.0× |   0.5× |
|------------|--------|--------|
| raw        |    -0.1 |    -1.3 |
| high-pass  |     1.8 |     1.8 |
| spectral   |    18.9 |    88.2 |
| dwt-band   |     2.5 |     3.6 |
| oracle Δ   |    18.3 |    51.5 |

Heatmaps (spectral-whitened autocorr): `autocorr_b512_s100.png` (1.0×), `_s50.png`
(0.5×), `_s25.png` (0.25×) — the lattice should sharpen as scale drops.
