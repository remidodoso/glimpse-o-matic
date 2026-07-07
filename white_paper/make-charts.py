#!/usr/bin/env python3
"""Generate the white-paper's data-driven charts as standalone SVGs.

Reads the CSVs the figure emitters produce (white_paper/figures/*.csv) and writes
chart_*.svg alongside them. The SVGs use a transparent background and mid-tone colors
that read on both the light and dark document themes, and are referenced from
white_paper.html like any other figure (dumbnail + lightbox).

Re-run after the emitters regenerate the CSVs:  python white_paper/make-charts.py
"""
import csv
from pathlib import Path

FIG = Path(__file__).resolve().parent / "figures"

# Theme-neutral palette (readable on both the warm-paper light theme and the dark theme).
TEXT = "#8c8475"
AXIS = "#9c9488"
GRID = "#9c9488"
A = "#b9802f"   # series A — accent (burlywood/amber)
B = "#5b8a90"   # series B — slate teal
ZERO = "#9c9488"

W, H = 720, 340
ML, MR, MT, MB = 56, 18, 20, 46          # margins
PW, PH = W - ML - MR, H - MT - MB         # plot area


def _read(name):
    with open(FIG / name, newline="") as f:
        rows = list(csv.reader(f))
    return rows[0], [[float(x) for x in r] for r in rows[1:] if r]


def _hdr(title, sub=None):
    s = [f'<svg class="wpchart" viewBox="0 0 {W} {H}" xmlns="http://www.w3.org/2000/svg" '
         f'role="img" aria-label="{title}">',
         '<style>'
         '.wpchart text{font-family:system-ui,-apple-system,"Segoe UI",sans-serif;}'
         f'.t{{fill:{TEXT};font-size:13px;}}.tl{{fill:{TEXT};font-size:12px;}}'
         '.ti{font-size:14px;font-weight:600;}'
         '</style>']
    s.append(f'<text class="t ti" x="{ML}" y="14">{title}</text>')
    if sub:
        s.append(f'<text class="tl" x="{W-MR}" y="14" text-anchor="end">{sub}</text>')
    return s


def _frame(s):
    # left + bottom axis
    s.append(f'<line x1="{ML}" y1="{MT}" x2="{ML}" y2="{MT+PH}" stroke="{AXIS}" stroke-width="1"/>')
    s.append(f'<line x1="{ML}" y1="{MT+PH}" x2="{ML+PW}" y2="{MT+PH}" stroke="{AXIS}" stroke-width="1"/>')


def _poly(xs, ys, color, xmin, xmax, ymin, ymax, width=1.8):
    def px(x): return ML + PW * (x - xmin) / (xmax - xmin)
    def py(y): return MT + PH * (1 - (y - ymin) / (ymax - ymin))
    pts = " ".join(f"{px(x):.1f},{py(y):.1f}" for x, y in zip(xs, ys))
    return f'<polyline fill="none" stroke="{color}" stroke-width="{width}" points="{pts}"/>'


def _legend(s, items, x, y):
    for i, (label, color) in enumerate(items):
        yy = y + i * 18
        s.append(f'<line x1="{x}" y1="{yy}" x2="{x+22}" y2="{yy}" stroke="{color}" stroke-width="3"/>')
        s.append(f'<text class="tl" x="{x+28}" y="{yy+4}">{label}</text>')


def save(name, lines):
    lines.append("</svg>")
    (FIG / name).write_text("\n".join(lines), encoding="utf-8")
    print("wrote", name)


# ── 1. Correlation tower ─────────────────────────────────────────────────────
def chart_corr():
    _, rows = _read("corr_gain.csv")
    marked = [r[2] for r in rows]
    unmarked = [r[3] for r in rows]
    n = len(rows)
    ymax = 0.9
    s = _hdr("Per-bit correlation: signal vs. noise floor", "192 payload bits")
    # gridlines + y ticks at -0.5, 0, 0.5
    def py(v): return MT + PH * (1 - (v + ymax) / (2 * ymax))
    for v in (-0.5, 0.0, 0.5):
        y = py(v)
        s.append(f'<line x1="{ML}" y1="{y:.1f}" x2="{ML+PW}" y2="{y:.1f}" stroke="{GRID}" '
                 f'stroke-width="1" opacity="{0.5 if v==0 else 0.28}"/>')
        s.append(f'<text class="tl" x="{ML-8}" y="{y+4:.1f}" text-anchor="end">{v:+.1f}</text>')
    bw = PW / n
    def bx(i): return ML + bw * (i + 0.5)
    y0 = py(0.0)
    # unmarked first (muted), then marked on top
    for i, v in enumerate(unmarked):
        y = py(max(-ymax, min(ymax, v)))
        s.append(f'<rect x="{bx(i)-bw*0.35:.1f}" y="{min(y,y0):.1f}" width="{bw*0.7:.1f}" '
                 f'height="{abs(y-y0):.1f}" fill="{B}" opacity="0.65"/>')
    for i, v in enumerate(marked):
        y = py(max(-ymax, min(ymax, v)))
        s.append(f'<rect x="{bx(i)-bw*0.35:.1f}" y="{min(y,y0):.1f}" width="{bw*0.7:.1f}" '
                 f'height="{abs(y-y0):.1f}" fill="{A}"/>')
    _frame(s)
    s.append(f'<text class="t" x="{ML+PW/2}" y="{H-10}" text-anchor="middle">payload bit (0–191), bar = correlation, sign = bit value</text>')
    _legend(s, [("watermarked image", A), ("un-watermarked image", B)], ML + 14, MT + 16)
    save("chart_corr.svg", s)


# ── 2. Autocorrelation: tile period moves with scale ─────────────────────────
def chart_autocorr():
    _, full = _read("autocorr_wm_1.0x_profile.csv")
    _, half = _read("autocorr_wm_0.5x_profile.csv")
    lo, hi = 24, 520
    fx = [r[0] for r in full if lo <= r[0] <= hi]
    fy = [r[1] for r in full if lo <= r[0] <= hi]
    hx = [r[0] for r in half if lo <= r[0] <= hi]
    hy = [r[1] for r in half if lo <= r[0] <= hi]
    ymax = max(max(fy), max(hy)) * 1.12
    s = _hdr("The watermark's period, and how it moves with scale")
    # period markers at 128 (half) and 256 (full)
    def px(x): return ML + PW * (x - lo) / (hi - lo)
    for lag, lab, col in ((128, "½× → 128", B), (256, "1× → 256", A)):
        x = px(lag)
        s.append(f'<line x1="{x:.1f}" y1="{MT}" x2="{x:.1f}" y2="{MT+PH}" stroke="{col}" '
                 f'stroke-width="1" stroke-dasharray="4 4" opacity="0.6"/>')
        s.append(f'<text class="tl" x="{x:.1f}" y="{MT+12}" text-anchor="middle" fill="{col}">{lab}</text>')
    for lag in (128, 256, 384, 512):
        x = px(lag)
        s.append(f'<text class="tl" x="{x:.1f}" y="{MT+PH+18}" text-anchor="middle">{lag}</text>')
    s.append(_poly(hx, hy, B, lo, hi, 0, ymax))
    s.append(_poly(fx, fy, A, lo, hi, 0, ymax))
    _frame(s)
    s.append(f'<text class="t" x="{ML+PW/2}" y="{H-8}" text-anchor="middle">lag (pixels)</text>')
    _legend(s, [("1.0× (full size)", A), ("0.5× (half size)", B)], ML + PW - 150, MT + 16)
    save("chart_autocorr.svg", s)


# ── 3. Needle vs. comb ───────────────────────────────────────────────────────
def chart_needle():
    _, rows = _read("needle_vs_comb.csv")
    xs = [r[0] for r in rows]
    pn = [r[1] for r in rows]
    st = [r[2] for r in rows]
    lo, hi = 0, max(xs)
    ymin, ymax = -0.5, 1.08
    s = _hdr("Self-agreement: a needle vs. a comb")
    def py(v): return MT + PH * (1 - (v - ymin) / (ymax - ymin))
    for v in (0.0, 0.5, 1.0):
        y = py(v)
        s.append(f'<line x1="{ML}" y1="{y:.1f}" x2="{ML+PW}" y2="{y:.1f}" stroke="{GRID}" '
                 f'stroke-width="1" opacity="{0.5 if v==0 else 0.28}"/>')
        s.append(f'<text class="tl" x="{ML-8}" y="{y+4:.1f}" text-anchor="end">{v:.1f}</text>')
    s.append(_poly(xs, st, B, lo, hi, ymin, ymax, 1.5))
    s.append(_poly(xs, pn, A, lo, hi, ymin, ymax, 1.8))
    _frame(s)
    s.append(f'<text class="t" x="{ML+PW/2}" y="{H-8}" text-anchor="middle">offset (pixels)</text>')
    _legend(s, [("noise tile (PN)", A), ("striped tile", B)], ML + PW - 150, MT + 16)
    save("chart_needle.svg", s)


# ── 4. Fence autocorrelation (the everyday-periodicity demo) ─────────────────
def chart_fence():
    _, rows = _read("autocorr_fence_profile.csv")
    xs = [r[0] for r in rows]
    ys = [r[1] for r in rows]
    lo, hi = 0, max(xs)
    ymin = min(0.0, min(ys)) * 1.05
    ymax = max(ys) * 1.08
    s = _hdr("Autocorrelation of a periodic pattern")
    def py(v): return MT + PH * (1 - (v - ymin) / (ymax - ymin))
    y0 = py(0.0)
    s.append(f'<line x1="{ML}" y1="{y0:.1f}" x2="{ML+PW}" y2="{y0:.1f}" stroke="{GRID}" stroke-width="1" opacity="0.5"/>')
    s.append(_poly(xs, ys, A, lo, hi, ymin, ymax))
    _frame(s)
    s.append(f'<text class="t" x="{ML+PW/2}" y="{H-8}" text-anchor="middle">lag (pixels) — peaks mark the repeat spacing</text>')
    save("chart_fence.svg", s)


# ── 5. Goldilocks tuning curve (bonus) ───────────────────────────────────────
def chart_tuning():
    _, rows = _read("goldilocks_tuning.csv")
    alpha = [r[0] for r in rows]
    psnr = [r[1] for r in rows]
    margin = [r[3] for r in rows]
    ax0, ax1 = min(alpha), max(alpha)
    m0, m1 = 0.0, 2.5          # left axis: margin
    p0, p1 = 30.0, 62.0        # right axis: PSNR
    s = _hdr("Strength α: decode margin vs. perceptual cost")
    def px(a): return ML + PW * (a - ax0) / (ax1 - ax0)
    def pym(v): return MT + PH * (1 - (v - m0) / (m1 - m0))
    def pyp(v): return MT + PH * (1 - (v - p0) / (p1 - p0))
    # left ticks (margin)
    for v in (0, 1, 2):
        y = pym(v)
        s.append(f'<line x1="{ML}" y1="{y:.1f}" x2="{ML+PW}" y2="{y:.1f}" stroke="{GRID}" stroke-width="1" opacity="0.22"/>')
        s.append(f'<text class="tl" x="{ML-8}" y="{y+4:.1f}" text-anchor="end" fill="{A}">{v}</text>')
    # right ticks (PSNR)
    for v in (30, 40, 50, 60):
        y = pyp(v)
        s.append(f'<text class="tl" x="{ML+PW+8}" y="{y+4:.1f}" fill="{B}">{v}</text>')
    # production alpha marker
    xp = px(0.15)
    s.append(f'<line x1="{xp:.1f}" y1="{MT}" x2="{xp:.1f}" y2="{MT+PH}" stroke="{ZERO}" stroke-width="1" stroke-dasharray="4 4" opacity="0.6"/>')
    s.append(f'<text class="tl" x="{xp+4:.1f}" y="{MT+PH-6:.1f}">α=0.15 (production)</text>')
    # series
    s.append(_poly(alpha, margin, A, ax0, ax1, m0, m1))
    s.append(_poly(alpha, psnr, B, ax0, ax1, p0, p1))
    for a, v in zip(alpha, margin):
        s.append(f'<circle cx="{px(a):.1f}" cy="{pym(v):.1f}" r="2.6" fill="{A}"/>')
    for a, v in zip(alpha, psnr):
        s.append(f'<circle cx="{px(a):.1f}" cy="{pyp(v):.1f}" r="2.6" fill="{B}"/>')
    # x ticks
    for a in (0.05, 0.15, 0.35, 0.60):
        s.append(f'<text class="tl" x="{px(a):.1f}" y="{MT+PH+18}" text-anchor="middle">{a:.2f}</text>')
    _frame(s)
    s.append(f'<text class="t" x="{ML+PW/2}" y="{H-8}" text-anchor="middle">embedding strength α</text>')
    _legend(s, [("decode margin (left)", A), ("PSNR dB (right)", B)], ML + 14, MT + 16)
    save("chart_tuning.svg", s)


# ── 6. Whitening: the buried peak, surfaced ──────────────────────────────────
def chart_whitening():
    _, rows = _read("whitening_profile.csv")
    lo, hi = 24, 520
    xs = [r[0] for r in rows if lo <= r[0] <= hi]
    raw = [r[1] for r in rows if lo <= r[0] <= hi]
    wht = [r[2] for r in rows if lo <= r[0] <= hi]
    # Wildly different units (raw autocorr vs whitened): normalize each to its own peak.
    rmax, wmax = max(raw), max(wht)
    raw = [v / rmax for v in raw]
    wht = [v / wmax for v in wht]
    ymin, ymax = min(min(raw), min(wht), -0.05) * 1.1, 1.1
    s = _hdr("One lag profile, before and after whitening", "each normalized to its own peak")
    def px(x): return ML + PW * (x - lo) / (hi - lo)
    x = px(256)
    s.append(f'<line x1="{x:.1f}" y1="{MT}" x2="{x:.1f}" y2="{MT+PH}" stroke="{A}" '
             f'stroke-width="1" stroke-dasharray="4 4" opacity="0.6"/>')
    s.append(f'<text class="tl" x="{x:.1f}" y="{MT+12}" text-anchor="middle" fill="{A}">tile period → 256</text>')
    for lag in (128, 256, 384, 512):
        s.append(f'<text class="tl" x="{px(lag):.1f}" y="{MT+PH+18}" text-anchor="middle">{lag}</text>')
    s.append(_poly(xs, raw, B, lo, hi, ymin, ymax, 1.5))
    s.append(_poly(xs, wht, A, lo, hi, ymin, ymax, 1.8))
    _frame(s)
    s.append(f'<text class="t" x="{ML+PW/2}" y="{H-8}" text-anchor="middle">lag (pixels)</text>')
    _legend(s, [("whitened", A), ("raw", B)], ML + PW - 120, MT + 16)
    save("chart_whitening.svg", s)


# ── 7. Whitening's blind region: the upscaled capture, full-res vs ½-pyramid ─
def chart_whitening_upscale():
    _, rows = _read("whitening_upscale_profile.csv")
    lo, hi = 24, 520
    xs = [r[0] for r in rows if lo <= r[0] <= hi]
    full = [r[1] for r in rows if lo <= r[0] <= hi]
    half = [r[2] for r in rows if lo <= r[0] <= hi]
    fmax, hmax = max(full), max(half)
    full = [v / fmax for v in full]
    half = [v / hmax for v in half]
    ymin, ymax = min(min(full), min(half), -0.05) * 1.1, 1.1
    s = _hdr("An upscaled capture: buried at full resolution, first at half",
             "each normalized to its own peak")
    def px(x): return ML + PW * (x - lo) / (hi - lo)
    for lag, lab, col in ((191, "½-pyramid → 191 (rank #1)", B),
                          (382, "true period at 1.49× → 382 (rank #21)", A)):
        x = px(lag)
        s.append(f'<line x1="{x:.1f}" y1="{MT}" x2="{x:.1f}" y2="{MT+PH}" stroke="{col}" '
                 f'stroke-width="1" stroke-dasharray="4 4" opacity="0.6"/>')
        s.append(f'<text class="tl" x="{x:.1f}" y="{MT+12}" text-anchor="middle" fill="{col}">{lab}</text>')
    for lag in (128, 256, 384, 512):
        s.append(f'<text class="tl" x="{px(lag):.1f}" y="{MT+PH+18}" text-anchor="middle">{lag}</text>')
    s.append(_poly(xs, full, A, lo, hi, ymin, ymax, 1.5))
    s.append(_poly(xs, half, B, lo, hi, ymin, ymax, 1.8))
    _frame(s)
    s.append(f'<text class="t" x="{ML+PW/2}" y="{H-8}" text-anchor="middle">lag (pixels), whitened profiles</text>')
    _legend(s, [("full resolution", A), ("½-pyramid level", B)], ML + 14, MT + 16)
    save("chart_whitening_upscale.svg", s)


if __name__ == "__main__":
    chart_corr()
    chart_autocorr()
    chart_needle()
    chart_fence()
    chart_tuning()
    chart_whitening()
    chart_whitening_upscale()
    print("charts ->", FIG)
