# Phase-7 investigation — brute-force scale sweep on failing crops

For each `tests/failed_crops/*.jpg`, a fine CRC/ECC-gated scale sweep (0.30–1.00, 0.2%
step) via `register_decode` (its own offset recovery), bypassing the autocorr *ranking*.
**RECOVERED** = some scale CRC-verified ⇒ recoverable; the autocorr just couldn't rank/hit
it (→ better coarse detection). **not recovered** ⇒ no scale verified ⇒ a fold-SNR floor at
that crop size. `fold-tiles` = ⌊tw/512⌋·⌊th/512⌋ at the reported scale (more = stronger fold).

| crop | size | result | scale | prominence | ECC | rescaled | fold-tiles |
|------|------|--------|-------|-----------|-----|----------|------------|
| original.jpg   | 3200×2133 | **RECOVERED** | 0.500 | 8.9 | 0 | 6400×4266 | 96 |
| sstest10.jpg   | 883×1338 | **RECOVERED** | 0.422 | 5.2 | 0 | 2092×3171 | 24 |
| sstest13.jpg   | 849×875 | **RECOVERED** | 0.506 | 4.7 | 0 | 1678×1729 | 9 |
| sstest15.jpg   | 1185×725 | **RECOVERED** | 0.560 | 6.6 | 0 | 2116×1295 | 8 |
| sstest16.jpg   | 855×1100 | **RECOVERED** | 0.558 | 4.7 | 0 | 1532×1971 | 6 |

