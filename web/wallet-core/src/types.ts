import { Data } from 'effect';

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

export const DEFAULT_MIN_CONFIRMATIONS = 64;
export const DEFAULT_MAX_STALENESS_BLOCKS = 900;
export const DEFAULT_REQUIRE_VERIFIED = true;

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

export type VerificationStatus =
  | 'hash_mismatch'
  | 'snapshot_in_future'
  | 'too_recent_reorg_risk'
  | 'too_stale'
  | 'not_finalized'
  | 'chain_id_mismatch'
  | 'helios_error';

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

export class VerificationError extends Data.TaggedError('VerificationError')<{
  readonly status: VerificationStatus;
  readonly message: string;
  readonly details?: Record<string, unknown>;
}> {}

export class HashMismatchError extends Data.TaggedError('HashMismatchError')<{
  readonly expected: string;
  readonly actual: string;
  readonly blockNumber: bigint;
}> {}

export class SnapshotInFutureError extends Data.TaggedError('SnapshotInFutureError')<{
  readonly snapshotBlock: bigint;
  readonly currentBlock: bigint;
}> {}

export class TooRecentError extends Data.TaggedError('TooRecentError')<{
  readonly depth: bigint;
  readonly minRequired: number;
}> {}

export class TooStaleError extends Data.TaggedError('TooStaleError')<{
  readonly depth: bigint;
  readonly maxAllowed: number;
}> {}

export class NotFinalizedError extends Data.TaggedError('NotFinalizedError')<{
  readonly snapshotBlock: bigint;
  readonly finalizedBlock: bigint;
}> {}

export class ChainIdMismatchError extends Data.TaggedError('ChainIdMismatchError')<{
  readonly expected: number;
  readonly actual: number;
}> {}

export class HeliosError extends Data.TaggedError('HeliosError')<{
  readonly cause: unknown;
}> {}

export class PirQueryError extends Data.TaggedError('PirQueryError')<{
  readonly message: string;
  readonly cause?: unknown;
}> {}

export class AddressNotFoundError extends Data.TaggedError('AddressNotFoundError')<{
  readonly address: string;
}> {}

export class NotInitializedError extends Data.TaggedError('NotInitializedError')<{
  readonly _brand?: never;
}> {}

export class SnapshotNotVerifiedError extends Data.TaggedError('SnapshotNotVerifiedError')<{
  readonly _brand?: never;
}> {}

export type SnapshotError =
  | VerificationError
  | HashMismatchError
  | SnapshotInFutureError
  | TooRecentError
  | TooStaleError
  | NotFinalizedError
  | ChainIdMismatchError
  | HeliosError
  | NotInitializedError
  | SnapshotNotVerifiedError;

export type BalanceError =
  | PirQueryError
  | AddressNotFoundError
  | SnapshotError;

export const BALANCE_RECORD_SIZE = 64;

export const NETWORK_CHAIN_IDS: Record<string, number> = {
  mainnet: 1,
  holesky: 17000,
  sepolia: 11155111,
};
