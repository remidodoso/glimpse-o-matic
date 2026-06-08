# Phase-7 — scale-peak ranking (single centre block vs multi-block sum)

Where the *true* tile period (lag = scale·256, from brute_scale.md) ranks among the top-12
whitened-autocorr peaks.  `absent` = not a top-12 peak.  The stdout dump shows each peak as
`lag→scale`; a ½×/⅓× harmonic relationship to a stronger peak is visible in the scale column
(e.g. a strong 1.01 with the true 0.51 = its half).

| crop | true scale | single-block rank | multi-block rank |
|------|-----------|-------------------|------------------|
| original.jpg   | 1.000 | #1 | #1 |
| sstest10.jpg   | 0.422 | #2 | #2 |
| sstest13.jpg   | 0.506 | absent | absent |
| sstest15.jpg   | 0.560 | absent | #9 |
| sstest16.jpg   | 0.558 | #4 | #4 |

