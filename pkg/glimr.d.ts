/* tslint:disable */
/* eslint-disable */

export class GlimrRenderer {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Draw image at `index` onto the photo canvas.
     * `offset` is the slide drag offset in CSS pixels:
     *   > 0 → dragging right (prev image enters from left)
     *   < 0 → dragging left  (next image enters from right)
     */
    draw(index: number, offset: number): void;
    /**
     * Draws the `<` / `>` hover arrow directly onto `self.canvas` (on top of the blitted image).
     * `zone`    — "left", "right", or "" (no-op).
     * `opacity` — current animation opacity (0.0–1.0); no-op if ≤ 0.
     * `index`   — current image index; used to show `>>` / `<<` at gallery boundaries.
     */
    draw_hover_indicator(index: number, zone: string, opacity: number): void;
    /**
     * Draws a thumbnail for image `index` into a caller-supplied canvas element.
     * Sets canvas width/height to match the scaled dimensions, then blits.
     * `carousel_size` — target size in CSS px on the constrained axis.
     * `fit_to_width`  — true in landscape (vertical strip); false in portrait (horizontal strip).
     */
    draw_thumbnail(canvas: HTMLCanvasElement, index: number, carousel_size: number, fit_to_width: boolean): void;
    /**
     * Renders image `index` in zoom/pan mode.
     * `scale`  — zoom factor (1.0 = 1:1 pixels, fit_scale = fully zoomed out)
     * `pan_x/y` — top-left corner of the viewport window in image-space pixels
     */
    draw_zoomed(index: number, scale: number, pan_x: number, pan_y: number): void;
    image_count(): number;
    /**
     * Size of the stored (XOR-decoded) JPEG/PNG bytes for image i.
     */
    image_file_size(i: number): number;
    /**
     * Decoded pixel height; 0 if image i has not been drawn yet.
     */
    image_height(i: number): number;
    image_name(i: number): string;
    /**
     * Decoded pixel width; 0 if image i has not been drawn yet.
     */
    image_width(i: number): number;
    load_zip(zip_bytes: Uint8Array): void;
    constructor(canvas: HTMLCanvasElement, backing: HTMLCanvasElement);
    /**
     * Returns the XOR-decoded JPEG/PNG bytes for image i.
     * JS uses this for the download button (one-shot blob URL, revoked immediately after click).
     */
    raw_bytes(i: number): Uint8Array;
}

/**
 * Exported so JS can emit timestamped log lines in the same format.
 */
export function glimr_log(func: string, msg: string): void;

export function xor_decode(input: Uint8Array): Uint8Array;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_glimrrenderer_free: (a: number, b: number) => void;
    readonly glimr_log: (a: number, b: number, c: number, d: number) => void;
    readonly glimrrenderer_draw: (a: number, b: number, c: number) => [number, number];
    readonly glimrrenderer_draw_hover_indicator: (a: number, b: number, c: number, d: number, e: number) => [number, number];
    readonly glimrrenderer_draw_thumbnail: (a: number, b: any, c: number, d: number, e: number) => [number, number];
    readonly glimrrenderer_draw_zoomed: (a: number, b: number, c: number, d: number, e: number) => [number, number];
    readonly glimrrenderer_image_count: (a: number) => number;
    readonly glimrrenderer_image_file_size: (a: number, b: number) => number;
    readonly glimrrenderer_image_height: (a: number, b: number) => number;
    readonly glimrrenderer_image_name: (a: number, b: number) => [number, number];
    readonly glimrrenderer_image_width: (a: number, b: number) => number;
    readonly glimrrenderer_load_zip: (a: number, b: number, c: number) => [number, number];
    readonly glimrrenderer_new: (a: any, b: any) => [number, number, number];
    readonly glimrrenderer_raw_bytes: (a: number, b: number) => [number, number];
    readonly xor_decode: (a: number, b: number) => [number, number];
    readonly __wbindgen_exn_store: (a: number) => void;
    readonly __externref_table_alloc: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __externref_table_dealloc: (a: number) => void;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
