# build-figures.ps1 (WP8) — regenerate all white-paper figures.
#
# Runs the #[ignore] WP-series emitter tests in glimr, release mode, capturing the
# stdout numbers (PSNR, max|Δ|, peaks, gain ratios, kernel grids) for figure captions.
# Output → white_paper/figures/ (image-derived figures at source resolution; synthetic
# tiles upscaled to 1024 on the long edge). Deterministic: fixed WM_KEY + PHASE3_PAYLOAD.
#
# Usage:  ./white_paper/build-figures.ps1
#
# Built once with --features registration: WP1 (wp_goldilocks) decodes through a channel
# via the blind decoder, which is registration-gated; the other emitters compile fine with
# the feature on, so one feature set = one compile.

# No global ErrorActionPreference='Stop': cargo writes progress to stderr, which PS 5.1
# would wrap as terminating errors if this script's output is piped. Real failures are
# caught via $LASTEXITCODE after each run.
Set-Location (Join-Path $PSScriptRoot '..') -ErrorAction Stop   # repo root (cargo workspace)

$emitters = @(
    'wp_goldilocks',        # WP1 — too-weak / Goldilocks / too-strong + tuning CSV (registration)
    'wp_triptych',          # WP2 — imperceptibility triptych (orig | marked | ×20 residual)
    'wp_keyed_pattern',     # WP3 — PN tile, weighted sum, spatial template
    'wp_dwt_decomp',        # WP4 — Mallat subband layout, embed bands outlined
    'wp_masking_map',       # WP5 — perceptual-masking gain heat overlay
    'wp_wavelet_residuals', # WP6 — Haar vs CDF 5/3 residual texture (riley) + 1:1 eye crops
    'wp_corr_gain',         # WP7 — per-bit correlation (marked vs unmarked) → corr_gain.csv
    'wp_filter_zoo',        # WP9 — convolution explainer on riley + locator + 1:1 eye crops
    'wp_sparsity_strip',    # WP10 — pixel vs wavelet (vs fft) sparsity
    'wp_band_knockout',     # WP11 — detail-level knockout strip
    'wp_autocorr',          # WP12 — autocorrelation figures (fence + wm profiles + surface)
    'wp_needle_vs_comb',    # WP13 — PN needle vs stripe comb
    'wp_stereogram',        # WP14 — random-dot autostereogram
    'wp_whitening',         # WP15 — whitening: input/whitened PNGs + raw-vs-whitened + upscale CSVs
    'wp_real_capture'       # WP16 — real-capture blind decodes (sstest51 + downscaled), published
)

# Note: WP17 (synthetic WebP channel) is intentionally NOT here — WebP encoding is
# deliberately not part of the framework. Its measurement was produced one-time with a
# borrowed libwebp encoder and recorded in feedback.md; see the WP17 note there.

foreach ($t in $emitters) {
    Write-Host "== $t ==" -ForegroundColor Cyan
    cargo test -p glimr --features registration --release $t -- --ignored --nocapture
    if ($LASTEXITCODE -ne 0) { throw "$t failed (exit $LASTEXITCODE)" }
}

Write-Host "`nAll figures regenerated -> white_paper/figures/" -ForegroundColor Green
