# Blind `--auto` sweep — multi-image robustness

_Generated 2026-06-11 02:01:12 -07:00 · glimr 0.1.0 · commit `155e3ce-dirty` · config ALPHA=0.15, levels=[2, 3], mask=0.5, ECC=BCH(192,160) t=4._

Each row drives the **production blind decoder** (`decode_blind_auto`, the same call the
`watermark-decode` tool makes) through the realistic capture chain, given no hint of scale or crop:

> embed in RGB (CDF 5/3, ALPHA=0.15, levels [2, 3], mask 0.5) → **scale** (display) → **crop** (screenshot) → **encode** (save) → `decode_blind_auto`

The watermark is composited in uncompressed RGB (as on screen) and meets compression only at the
screenshot-save step, so `enc` is the *save format*; the source photo's own JPEG history is
irrelevant.  The matrix is defined in [`tests/blind_sweep.yaml`](../blind_sweep.yaml) — one line per case.

**52/53 CRC-verified · 42/53 clean (no ECC) · 10 used ECC · 0 needed refine · 1 needed Chase · 1 skipped.**
**Decode time (production path, release): median 3.0 s, max 53.0 s.**
Per-decode split (median): templates 0 ms + FFT 591 ms + search 2.4 s; one-time template setup peaks at 3.9 s.

Regenerate: `cargo test -p glimr --features registration --release blind_auto_sweep -- --ignored --nocapture`

### Column legend

| column | meaning |
|---|---|
| **image** | source fixture (stem). |
| **enc** | screenshot save format: `raw` = lossless (PNG-equivalent), `q<NN>` = JPEG quality NN, `w<NN>` = WebP (unimplemented → skipped). |
| **scale** | display scale applied before capture (`1.00` native, `0.50` half, `1.50` enlarged). |
| **crop** | pixels cropped from each edge *after* scaling, `L:T:R:B` (or `none`). |
| **recovered** | scale the blind decoder locked onto; `prim` = the true period, `harm` = a self-similar harmonic sibling (½×/⅓×). Both decode correctly — see note. |
| **prom** | phase-peak *prominence* of the winning candidate (peak ÷ median) — how decisively it stood out. High = a clean lock; a low value next to a deep `path` is a marginal recovery. |
| **path** | how it won: `C<r>/<n>` = coarse candidate *r* of *n*; `R c<k>` = needed the fine refine pass on candidate *k*. |
| **time** | wallclock of the decode call only (excludes channel-simulation setup) — the secondary figure of merit. |
| **ECC** | `clean` = raw CRC passed, no correction; `fixed N` = BCH repaired N bit errors; `FAIL` = CRC failed even after ECC. |
| **crc** | the verdict: `✓` = full 128-bit payload recovered exactly. The only pass/fail signal. |

### Why a `harm` recovery is not a failure

A `recovered` tagged `harm` means the decoder locked a *harmonic* of the true tile period (e.g.
reported 1.0× for a ½-size image) rather than the period itself.  Downscaling low-pass-filters the
mark, so the strongest autocorrelation peak is often a harmonic; the decoder expands each peak into
`{s, s/2, s/3}` siblings, and because the PN tiling is self-similar across them, decoding succeeds
perfectly via a sibling.  `crc ✓` is the verdict.

### Scope & future work

Variety is driven entirely by `tests/blind_sweep.yaml` — add lines to widen the envelope.  Still to
add as channel variables: small rotations, aspect changes, overlays, and additive noise.  WebP save
(`w<NN>`) is parsed but stubbed — it needs an external encoder (planned: wrap `ffmpeg`/`cwebp`), so
those cases report `skip`.

| image      | enc  | scale | crop         | recovered    | prom  | path    | time     | ECC      | crc |
|------------|------|-------|--------------|--------------|-------|---------|----------|----------|-----|
| collibrina | raw  |  1.00 | none         | 1.00x prim   |  11.9 | C1/4    |    6.5 s | clean    |  ✓  |
| fairbanks  | raw  |  1.00 | none         | 1.00x prim   |  10.3 | C1/2    |    2.9 s | clean    |  ✓  |
| quyen      | raw  |  1.00 | none         | 1.00x prim   |   8.4 | C1/5    |    3.0 s | clean    |  ✓  |
| riley      | raw  |  1.00 | none         | 1.00x prim   |   9.8 | C1/3    |    3.2 s | clean    |  ✓  |
| zia        | raw  |  1.00 | none         | 1.00x prim   |  10.9 | C1/4    |    2.9 s | clean    |  ✓  |
| collibrina | q90  |  1.00 | none         | 1.00x prim   |  11.5 | C1/4    |    3.1 s | clean    |  ✓  |
| fairbanks  | q90  |  1.00 | none         | 1.00x prim   |  10.0 | C1/3    |    3.1 s | clean    |  ✓  |
| quyen      | q90  |  1.00 | none         | 1.00x prim   |   7.6 | C1/5    |    2.9 s | clean    |  ✓  |
| riley      | q90  |  1.00 | none         | 1.00x prim   |   9.0 | C1/2    |    3.0 s | clean    |  ✓  |
| zia        | q90  |  1.00 | none         | 1.00x prim   |  10.6 | C1/3    |    2.9 s | clean    |  ✓  |
| collibrina | raw  |  0.50 | none         | 1.00x harm   |   3.9 | C1/6    |    2.5 s | clean    |  ✓  |
| fairbanks  | raw  |  0.50 | none         | 1.00x harm   |   3.6 | C1/6    |    2.6 s | clean    |  ✓  |
| quyen      | raw  |  0.50 | none         | 0.50x prim   |   6.6 | C2/7    |    4.2 s | clean    |  ✓  |
| riley      | raw  |  0.50 | none         | 0.50x prim   |   8.2 | C2/5    |    4.1 s | clean    |  ✓  |
| zia        | raw  |  0.50 | none         | 0.50x prim   |   8.5 | C2/6    |    3.9 s | clean    |  ✓  |
| collibrina | q90  |  0.50 | none         | 1.00x harm   |   3.5 | C1/6    |    2.5 s | fixed 3  |  ✓  |
| fairbanks  | q90  |  0.50 | none         | 1.00x harm   |   3.4 | C1/5    |    2.5 s | fixed 2  |  ✓  |
| quyen      | q90  |  0.50 | none         | 0.50x prim   |   5.3 | C2/7    |    4.0 s | fixed 1  |  ✓  |
| riley      | q90  |  0.50 | none         | 0.50x prim   |   6.5 | C2/6    |    4.1 s | clean    |  ✓  |
| zia        | q90  |  0.50 | none         | 0.50x prim   |   7.9 | C2/6    |    4.0 s | clean    |  ✓  |
| collibrina | q85  |  1.00 | none         | 1.00x prim   |  11.3 | C1/3    |    3.3 s | clean    |  ✓  |
| fairbanks  | q85  |  1.00 | none         | 1.00x prim   |   8.6 | C1/3    |    3.4 s | clean    |  ✓  |
| quyen      | q85  |  1.00 | none         | 1.00x prim   |   7.5 | C1/5    |    3.1 s | clean    |  ✓  |
| riley      | q85  |  1.00 | none         | 1.00x prim   |   8.7 | C1/2    |    3.0 s | clean    |  ✓  |
| zia        | q85  |  1.00 | none         | 1.00x prim   |   9.8 | C1/2    |    2.9 s | clean    |  ✓  |
| collibrina | q80  |  0.75 | none         | 0.75x prim   |  10.0 | C1/5    |    3.0 s | clean    |  ✓  |
| fairbanks  | q80  |  0.75 | none         | 0.75x prim   |   8.1 | C1/4    |    3.1 s | clean    |  ✓  |
| quyen      | q80  |  0.75 | none         | 0.75x prim   |   6.3 | C1/5    |    3.0 s | clean    |  ✓  |
| riley      | q80  |  0.75 | none         | 0.75x prim   |   5.7 | C1/3    |    3.4 s | clean    |  ✓  |
| zia        | q80  |  0.75 | none         | 0.75x prim   |   9.4 | C1/4    |    2.9 s | clean    |  ✓  |
| collibrina | q90  |  1.00 | 130:200:0:0  | 1.00x prim   |   9.7 | C1/4    |    2.9 s | clean    |  ✓  |
| fairbanks  | q90  |  1.00 | 130:200:0:0  | 1.00x prim   |   8.1 | C1/1    |    2.8 s | clean    |  ✓  |
| quyen      | q90  |  1.00 | 130:200:0:0  | 1.00x prim   |   5.8 | C1/5    |    2.8 s | clean    |  ✓  |
| riley      | q90  |  1.00 | 130:200:0:0  | 1.00x prim   |   7.2 | C2/3    |    4.7 s | clean    |  ✓  |
| zia        | q90  |  1.00 | 130:200:0:0  | 1.00x prim   |   8.7 | C1/3    |    2.8 s | clean    |  ✓  |
| collibrina | q85  |  0.66 | 60:60:60:60  | 0.66x prim   |   6.0 | C1/5    |    2.8 s | clean    |  ✓  |
| fairbanks  | q85  |  0.66 | 60:60:60:60  | 0.66x prim   |   4.6 | C1/4    |    2.8 s | clean    |  ✓  |
| quyen      | q85  |  0.66 | 60:60:60:60  | 0.66x prim   |   3.5 | C1/6    |    2.8 s | clean    |  ✓  |
| riley      | q85  |  0.66 | 60:60:60:60  | 0.66x prim   |   4.1 | C1/5    |    3.0 s | clean    |  ✓  |
| zia        | q85  |  0.66 | 60:60:60:60  | 0.66x prim   |   5.9 | C1/4    |    2.8 s | clean    |  ✓  |
| riley      | q80  |  0.90 | none         | 0.90x prim   |   4.9 | C1/4    |    3.0 s | clean    |  ✓  |
| riley      | q70  |  0.70 | none         | 0.70x prim   |   3.2 | C4/5    |    7.0 s | fixed 3  |  ✓  |
| riley      | q50  |  0.60 | none         | 0.60x prim   |   2.0 | Chase   |   38.2 s | fixed 4  |  ✓  |
| quyen      | q80  |  1.20 | none         | 1.20x prim   |   6.4 | C1/3    |    2.9 s | clean    |  ✓  |
| quyen      | q40  |  0.66 | 10:10:10:10  | 0.66x prim   |   4.0 | C2/9    |   10.4 s | fixed 3  |  ✓  |
| riley      | q80  |  0.90 | 100:100:100:100 | 0.90x prim   |   4.2 | C2/4    |    5.1 s | fixed 1  |  ✓  |
| riley      | q70  |  0.70 | 200:200:200:200 | 0.70x prim   |   4.4 | C4/7    |    6.6 s | fixed 2  |  ✓  |
| quyen      | q50  |  0.60 | 200:200:200:200 | 0.60x prim   |   2.5 | —       |   53.0 s | FAIL     |  ✗  |
| quyen      | q80  |  1.20 | none         | 1.20x prim   |   6.4 | C1/3    |    2.9 s | clean    |  ✓  |
| quyen      | q40  |  0.66 | none         | 0.66x prim   |   4.3 | C3/5    |    5.2 s | fixed 2  |  ✓  |
| quyen      | q90  |  0.37 | none         | 0.37x prim   |   4.0 | C2/8    |    3.7 s | fixed 3  |  ✓  |
| quyen      | q85  |  1.50 | none         | 1.50x prim   |   7.7 | C1/2    |    3.0 s | clean    |  ✓  |
| zia        | raw  |  0.66 | 100:0:100:0  | 0.66x prim   |   9.6 | C1/5    |    2.8 s | clean    |  ✓  |
| quyen      | w90  |  1.00 | none         | —            |     — | skip    |        — | webp n/i |  —  |

_The decode path is identical to the `watermark-decode` tool; this table is its behaviour and speed
across a configurable matrix of realistic captures._
