/* tslint:disable */
/* eslint-disable */

export function encode_balance_of(address: string): Uint8Array;

export function encode_transfer(to: string, amount: string): Uint8Array;

export function format_units(value: string, decimals: number): string;

export function generate_wallet(): string;

export function get_address(private_key: string): string;

export function keccak256(data: Uint8Array): Uint8Array;

export function parse_address(address: string): string;

export function parse_units(value: string, decimals: number): string;

export function sign_authorization(private_key: string, auth_request_json: string): string;

export function sign_message(private_key: string, message: string): string;

export function sign_typed_data_hash(private_key: string, hash_hex: string): string;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
  readonly memory: WebAssembly.Memory;
  readonly encode_balance_of: (a: number, b: number) => [number, number, number, number];
  readonly encode_transfer: (a: number, b: number, c: number, d: number) => [number, number, number, number];
  readonly format_units: (a: number, b: number, c: number) => [number, number, number, number];
  readonly generate_wallet: () => [number, number, number, number];
  readonly get_address: (a: number, b: number) => [number, number, number, number];
  readonly keccak256: (a: number, b: number) => [number, number];
  readonly parse_address: (a: number, b: number) => [number, number, number, number];
  readonly parse_units: (a: number, b: number, c: number) => [number, number, number, number];
  readonly sign_authorization: (a: number, b: number, c: number, d: number) => [number, number, number, number];
  readonly sign_message: (a: number, b: number, c: number, d: number) => [number, number, number, number];
  readonly sign_typed_data_hash: (a: number, b: number, c: number, d: number) => [number, number, number, number];
  readonly __wbindgen_exn_store: (a: number) => void;
  readonly __externref_table_alloc: () => number;
  readonly __wbindgen_externrefs: WebAssembly.Table;
  readonly __wbindgen_malloc: (a: number, b: number) => number;
  readonly __wbindgen_free: (a: number, b: number, c: number) => void;
  readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
  readonly __externref_table_dealloc: (a: number) => void;
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
