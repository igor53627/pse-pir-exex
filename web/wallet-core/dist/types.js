import { Data } from 'effect';
export const DEFAULT_MIN_CONFIRMATIONS = 64;
export const DEFAULT_MAX_STALENESS_BLOCKS = 900;
export const DEFAULT_REQUIRE_VERIFIED = true;
export class VerificationError extends Data.TaggedError('VerificationError') {
}
export class HashMismatchError extends Data.TaggedError('HashMismatchError') {
}
export class SnapshotInFutureError extends Data.TaggedError('SnapshotInFutureError') {
}
export class TooRecentError extends Data.TaggedError('TooRecentError') {
}
export class TooStaleError extends Data.TaggedError('TooStaleError') {
}
export class NotFinalizedError extends Data.TaggedError('NotFinalizedError') {
}
export class ChainIdMismatchError extends Data.TaggedError('ChainIdMismatchError') {
}
export class HeliosError extends Data.TaggedError('HeliosError') {
}
export class PirQueryError extends Data.TaggedError('PirQueryError') {
}
export class AddressNotFoundError extends Data.TaggedError('AddressNotFoundError') {
}
export class NotInitializedError extends Data.TaggedError('NotInitializedError') {
}
export class SnapshotNotVerifiedError extends Data.TaggedError('SnapshotNotVerifiedError') {
}
export const BALANCE_RECORD_SIZE = 64;
export const NETWORK_CHAIN_IDS = {
    mainnet: 1,
    holesky: 17000,
    sepolia: 11155111,
};
//# sourceMappingURL=types.js.map