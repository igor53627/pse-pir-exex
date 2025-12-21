export interface WalletCoreConfig {
    pirServerUrl: string;
    executionRpc: string;
    consensusRpc: string;
    network: 'mainnet' | 'holesky' | 'sepolia';
    checkpoint?: string;
    minConfirmationsForSafety?: number;
    maxSnapshotStalenessBlocks?: number;
    requireVerifiedSnapshot?: boolean;
}
export declare const DEFAULT_MIN_CONFIRMATIONS = 64;
export declare const DEFAULT_MAX_STALENESS_BLOCKS = 900;
export declare const DEFAULT_REQUIRE_VERIFIED = true;
export interface BalanceResult {
    address: string;
    ethBalance: bigint;
    usdcBalance: bigint;
    snapshotBlock: bigint;
    verified: boolean;
    source: 'pir' | 'rpc';
}
export interface BalanceMetadata {
    chainId: number;
    snapshotBlock: number;
    snapshotBlockHash: string;
    usdcContract: string;
    recordSize: number;
    numRecords: number;
    addresses: string[];
}
export type VerificationStatus = 'hash_mismatch' | 'snapshot_in_future' | 'too_recent_reorg_risk' | 'too_stale' | 'not_finalized' | 'chain_id_mismatch' | 'helios_error';
export interface VerificationResult {
    valid: boolean;
    verified: boolean;
    heliosBlockHash: string;
    expectedBlockHash: string;
    blockNumber: bigint;
    latestBlock?: bigint;
    finalizedBlock?: bigint;
    depthFromHead?: bigint;
    status: VerificationStatus[];
    error?: string;
}
declare const VerificationError_base: new <A extends Record<string, any> = {}>(args: import("effect/Types").Equals<A, {}> extends true ? void : { readonly [P in keyof A as P extends "_tag" ? never : P]: A[P]; }) => import("effect/Cause").YieldableError & {
    readonly _tag: "VerificationError";
} & Readonly<A>;
export declare class VerificationError extends VerificationError_base<{
    readonly status: VerificationStatus;
    readonly message: string;
    readonly details?: Record<string, unknown>;
}> {
}
declare const HashMismatchError_base: new <A extends Record<string, any> = {}>(args: import("effect/Types").Equals<A, {}> extends true ? void : { readonly [P in keyof A as P extends "_tag" ? never : P]: A[P]; }) => import("effect/Cause").YieldableError & {
    readonly _tag: "HashMismatchError";
} & Readonly<A>;
export declare class HashMismatchError extends HashMismatchError_base<{
    readonly expected: string;
    readonly actual: string;
    readonly blockNumber: bigint;
}> {
}
declare const SnapshotInFutureError_base: new <A extends Record<string, any> = {}>(args: import("effect/Types").Equals<A, {}> extends true ? void : { readonly [P in keyof A as P extends "_tag" ? never : P]: A[P]; }) => import("effect/Cause").YieldableError & {
    readonly _tag: "SnapshotInFutureError";
} & Readonly<A>;
export declare class SnapshotInFutureError extends SnapshotInFutureError_base<{
    readonly snapshotBlock: bigint;
    readonly currentBlock: bigint;
}> {
}
declare const TooRecentError_base: new <A extends Record<string, any> = {}>(args: import("effect/Types").Equals<A, {}> extends true ? void : { readonly [P in keyof A as P extends "_tag" ? never : P]: A[P]; }) => import("effect/Cause").YieldableError & {
    readonly _tag: "TooRecentError";
} & Readonly<A>;
export declare class TooRecentError extends TooRecentError_base<{
    readonly depth: bigint;
    readonly minRequired: number;
}> {
}
declare const TooStaleError_base: new <A extends Record<string, any> = {}>(args: import("effect/Types").Equals<A, {}> extends true ? void : { readonly [P in keyof A as P extends "_tag" ? never : P]: A[P]; }) => import("effect/Cause").YieldableError & {
    readonly _tag: "TooStaleError";
} & Readonly<A>;
export declare class TooStaleError extends TooStaleError_base<{
    readonly depth: bigint;
    readonly maxAllowed: number;
}> {
}
declare const NotFinalizedError_base: new <A extends Record<string, any> = {}>(args: import("effect/Types").Equals<A, {}> extends true ? void : { readonly [P in keyof A as P extends "_tag" ? never : P]: A[P]; }) => import("effect/Cause").YieldableError & {
    readonly _tag: "NotFinalizedError";
} & Readonly<A>;
export declare class NotFinalizedError extends NotFinalizedError_base<{
    readonly snapshotBlock: bigint;
    readonly finalizedBlock: bigint;
}> {
}
declare const ChainIdMismatchError_base: new <A extends Record<string, any> = {}>(args: import("effect/Types").Equals<A, {}> extends true ? void : { readonly [P in keyof A as P extends "_tag" ? never : P]: A[P]; }) => import("effect/Cause").YieldableError & {
    readonly _tag: "ChainIdMismatchError";
} & Readonly<A>;
export declare class ChainIdMismatchError extends ChainIdMismatchError_base<{
    readonly expected: number;
    readonly actual: number;
}> {
}
declare const HeliosError_base: new <A extends Record<string, any> = {}>(args: import("effect/Types").Equals<A, {}> extends true ? void : { readonly [P in keyof A as P extends "_tag" ? never : P]: A[P]; }) => import("effect/Cause").YieldableError & {
    readonly _tag: "HeliosError";
} & Readonly<A>;
export declare class HeliosError extends HeliosError_base<{
    readonly cause: unknown;
}> {
}
declare const PirQueryError_base: new <A extends Record<string, any> = {}>(args: import("effect/Types").Equals<A, {}> extends true ? void : { readonly [P in keyof A as P extends "_tag" ? never : P]: A[P]; }) => import("effect/Cause").YieldableError & {
    readonly _tag: "PirQueryError";
} & Readonly<A>;
export declare class PirQueryError extends PirQueryError_base<{
    readonly message: string;
    readonly cause?: unknown;
}> {
}
declare const AddressNotFoundError_base: new <A extends Record<string, any> = {}>(args: import("effect/Types").Equals<A, {}> extends true ? void : { readonly [P in keyof A as P extends "_tag" ? never : P]: A[P]; }) => import("effect/Cause").YieldableError & {
    readonly _tag: "AddressNotFoundError";
} & Readonly<A>;
export declare class AddressNotFoundError extends AddressNotFoundError_base<{
    readonly address: string;
}> {
}
declare const NotInitializedError_base: new <A extends Record<string, any> = {}>(args: import("effect/Types").Equals<A, {}> extends true ? void : { readonly [P in keyof A as P extends "_tag" ? never : P]: A[P]; }) => import("effect/Cause").YieldableError & {
    readonly _tag: "NotInitializedError";
} & Readonly<A>;
export declare class NotInitializedError extends NotInitializedError_base<{
    readonly _brand?: never;
}> {
}
declare const SnapshotNotVerifiedError_base: new <A extends Record<string, any> = {}>(args: import("effect/Types").Equals<A, {}> extends true ? void : { readonly [P in keyof A as P extends "_tag" ? never : P]: A[P]; }) => import("effect/Cause").YieldableError & {
    readonly _tag: "SnapshotNotVerifiedError";
} & Readonly<A>;
export declare class SnapshotNotVerifiedError extends SnapshotNotVerifiedError_base<{
    readonly _brand?: never;
}> {
}
export type SnapshotError = VerificationError | HashMismatchError | SnapshotInFutureError | TooRecentError | TooStaleError | NotFinalizedError | ChainIdMismatchError | HeliosError | NotInitializedError | SnapshotNotVerifiedError;
export type BalanceError = PirQueryError | AddressNotFoundError | SnapshotError;
export declare const BALANCE_RECORD_SIZE = 64;
export declare const NETWORK_CHAIN_IDS: Record<string, number>;
export {};
//# sourceMappingURL=types.d.ts.map