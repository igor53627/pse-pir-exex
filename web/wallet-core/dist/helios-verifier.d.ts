import { Effect } from 'effect';
import type { VerificationResult } from './types.js';
import { HashMismatchError, SnapshotInFutureError, TooRecentError, TooStaleError, NotFinalizedError, HeliosError } from './types.js';
export interface VerifierConfig {
    executionRpc: string;
    consensusRpc: string;
    network: 'mainnet' | 'holesky' | 'sepolia';
    checkpoint?: string;
    minConfirmationsForSafety?: number;
    maxSnapshotStalenessBlocks?: number;
}
export declare class HeliosVerifier {
    private provider;
    private config;
    private networkKind;
    private minConfirmations;
    private maxStaleness;
    constructor(config: VerifierConfig);
    init(): Promise<void>;
    getBlockHash(blockNumber: bigint): Promise<string>;
    getCurrentBlock(): Promise<bigint>;
    getFinalizedBlock(): Promise<bigint | undefined>;
    verifySnapshotEffect(expectedBlockNumber: bigint, expectedBlockHash: string): Effect.Effect<VerificationResult, HashMismatchError | SnapshotInFutureError | TooRecentError | TooStaleError | NotFinalizedError | HeliosError>;
    verifySnapshotBlock(expectedBlockNumber: bigint, expectedBlockHash: string): Promise<VerificationResult>;
}
//# sourceMappingURL=helios-verifier.d.ts.map