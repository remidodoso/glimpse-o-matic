/* tslint:disable */
/* eslint-disable */

export class GlimrRenderer {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Stores the zip bytes and resets parse state. Returns total byte count
     * so JS can compute progress as load_bytes_done() / total.
     */
    begin_zip_load(zip_bytes: Uint8Array): number;
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
     * Renders image `index` in zoom/pan mode.
     * `scale`  — zoom factor (1.0 = 1:1 pixels, fit_scale = fully zoomed out)
     * `pan_x/y` — top-left corner of the viewport window in image-space pixels
     */
    draw_zoomed(index: number, scale: number, pan_x: number, pan_y: number): void;
    /**
     * Sorts accumulated entries, populates names/image_bytes, frees the
     * buffered zip bytes. Call once load_next_entry returns Ok(true).
     */
    finish_zip_load(): void;
    /**
     * Returns the raw (XOR-decoded) bytes for image i as a Uint8Array.
     * JS passes these to createImageBitmap; the Blob is transient and never
     * stored as an accessible object.
     */
    get_image_bytes(i: number): Uint8Array;
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
    /**
     * Returns true if image i has been decoded and cached.
     */
    is_decoded(i: number): boolean;
    /**
     * Current byte position in the pending zip; divide by begin_zip_load's
     * return value to get extraction progress (0.0–1.0).
     */
    load_bytes_done(): number;
    /**
     * Parses one local file header. Returns Ok(false) while entries remain,
     * Ok(true) when the central directory is reached or the buffer is exhausted,
     * Err if the zip is malformed or unsupported.
     */
    load_next_entry(): boolean;
    load_zip(zip_bytes: Uint8Array): void;
    constructor(canvas: HTMLCanvasElement, backing: HTMLCanvasElement);
    /**
     * Returns the XOR-decoded JPEG/PNG bytes for image i.
     * JS uses this for the download button (one-shot blob URL, revoked immediately after click).
     */
    raw_bytes(i: number): Uint8Array;
    /**
     * Stores watermarked RGBA pixels for image i. Called by JS after
     * createImageBitmap → OffscreenCanvas → getImageData. Watermark
     * is applied here before caching.
     */
    receive_pixels(i: number, width: number, height: number, data: Uint8Array): void;
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
    readonly glimrrenderer_begin_zip_load: (a: number, b: number, c: number) => number;
    readonly glimrrenderer_draw: (a: number, b: number, c: number) => [number, number];
    readonly glimrrenderer_draw_hover_indicator: (a: number, b: number, c: number, d: number, e: number) => [number, number];
    readonly glimrrenderer_draw_zoomed: (a: number, b: number, c: number, d: number, e: number) => [number, number];
    readonly glimrrenderer_finish_zip_load: (a: number) => [number, number];
    readonly glimrrenderer_get_image_bytes: (a: number, b: number) => any;
    readonly glimrrenderer_image_count: (a: number) => number;
    readonly glimrrenderer_image_file_size: (a: number, b: number) => number;
    readonly glimrrenderer_image_height: (a: number, b: number) => number;
    readonly glimrrenderer_image_name: (a: number, b: number) => [number, number];
    readonly glimrrenderer_image_width: (a: number, b: number) => number;
    readonly glimrrenderer_is_decoded: (a: number, b: number) => number;
    readonly glimrrenderer_load_bytes_done: (a: number) => number;
    readonly glimrrenderer_load_next_entry: (a: number) => [number, number, number];
    readonly glimrrenderer_load_zip: (a: number, b: number, c: number) => [number, number];
    readonly glimrrenderer_new: (a: any, b: any) => [number, number, number];
    readonly glimrrenderer_raw_bytes: (a: number, b: number) => [number, number];
    readonly glimrrenderer_receive_pixels: (a: number, b: number, c: number, d: number, e: number, f: number) => [number, number];
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
