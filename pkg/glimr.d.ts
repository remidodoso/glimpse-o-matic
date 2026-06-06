/* tslint:disable */
/* eslint-disable */

export class GlimrRenderer {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Resets all image state and parser state. Call before feeding the first chunk.
     */
    begin_zip_stream(): void;
    /**
     * Draw image at `index` onto the photo canvas.
     */
    draw(index: number, offset: number): void;
    /**
     * Draws the hover arrow directly onto `self.canvas`.
     */
    draw_hover_indicator(index: number, zone: string, opacity: number): void;
    /**
     * Renders image `index` in zoom/pan mode.
     */
    draw_zoomed(index: number, scale: number, pan_x: number, pan_y: number): void;
    /**
     * Cap the watermarked-RGBA cache at PIXEL_CACHE_BUDGET bytes so a large
     * catalog can't grow `pixel_cache` without bound.  Evicts the cached image
     * farthest (by index) from `current` first, never evicting `current` itself.
     * Evicted images are simply re-decoded + re-watermarked if revisited.
     * JS calls this after each `receive_pixels`, anchored on the displayed image.
     */
    enforce_cache_budget(current: number): void;
    /**
     * Feed the next chunk of zip bytes. Advances the state machine as far as
     * possible, decompressing and XOR-decoding each complete entry. Returns
     * the total number of image entries ready so far. Errors on malformed zip.
     */
    feed_bytes(chunk: Uint8Array): number;
    /**
     * Returns the raw (XOR-decoded) bytes for image i as a Uint8Array.
     */
    get_image_bytes(i: number): Uint8Array;
    image_count(): number;
    image_file_size(i: number): number;
    image_height(i: number): number;
    image_name(i: number): string;
    image_width(i: number): number;
    /**
     * Returns true if image i has been decoded and cached.
     */
    is_decoded(i: number): boolean;
    /**
     * True once a central directory or end-of-archive signature has been seen.
     */
    is_stream_done(): boolean;
    constructor(canvas: HTMLCanvasElement, backing: HTMLCanvasElement);
    /**
     * Stores watermarked RGBA pixels for image i. Called by JS after
     * createImageBitmap → OffscreenCanvas → getImageData.
     * `payload` is the 16-byte watermark payload assembled by JS `build_payload()`.
     */
    receive_pixels(i: number, width: number, height: number, data: Uint8Array, payload: Uint8Array): void;
    /**
     * Watermarked RGBA pixels for image `i` at native resolution (for export).
     * Empty if the image hasn't been decoded/watermarked yet.  This is the only
     * full-resolution image data exposed to JS for download — it is always
     * watermarked; the un-watermarked source bytes are never handed out for export.
     */
    watermarked_pixels(i: number): Uint8Array;
}

/**
 * Exported so JS can emit timestamped log lines in the same format.
 */
export function glimr_log(func: string, msg: string): void;

/**
 * Exported for direct use where needed.
 */
export function xor_decode(input: Uint8Array): Uint8Array;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_glimrrenderer_free: (a: number, b: number) => void;
    readonly glimr_log: (a: number, b: number, c: number, d: number) => void;
    readonly glimrrenderer_begin_zip_stream: (a: number) => void;
    readonly glimrrenderer_draw: (a: number, b: number, c: number) => [number, number];
    readonly glimrrenderer_draw_hover_indicator: (a: number, b: number, c: number, d: number, e: number) => [number, number];
    readonly glimrrenderer_draw_zoomed: (a: number, b: number, c: number, d: number, e: number) => [number, number];
    readonly glimrrenderer_enforce_cache_budget: (a: number, b: number) => void;
    readonly glimrrenderer_feed_bytes: (a: number, b: number, c: number) => [number, number, number];
    readonly glimrrenderer_get_image_bytes: (a: number, b: number) => any;
    readonly glimrrenderer_image_count: (a: number) => number;
    readonly glimrrenderer_image_file_size: (a: number, b: number) => number;
    readonly glimrrenderer_image_height: (a: number, b: number) => number;
    readonly glimrrenderer_image_name: (a: number, b: number) => [number, number];
    readonly glimrrenderer_image_width: (a: number, b: number) => number;
    readonly glimrrenderer_is_decoded: (a: number, b: number) => number;
    readonly glimrrenderer_is_stream_done: (a: number) => number;
    readonly glimrrenderer_new: (a: any, b: any) => [number, number, number];
    readonly glimrrenderer_receive_pixels: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number) => [number, number];
    readonly glimrrenderer_watermarked_pixels: (a: number, b: number) => any;
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
