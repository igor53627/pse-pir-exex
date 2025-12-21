import { Effect, pipe } from 'effect';
import type { Config, NetworkKind, Network } from '@a16z/helios';
import { createHeliosProvider, HeliosProvider } from '@a16z/helios';

import type { VerificationResult, VerificationStatus } from './types.js';
import {
  HashMismatchError,
  SnapshotInFutureError,
  TooRecentError,
  TooStaleError,
  NotFinalizedError,
  HeliosError,
  DEFAULT_MIN_CONFIRMATIONS,
  DEFAULT_MAX_STALENESS_BLOCKS,
} from './types.js';

export interface VerifierConfig {
  executionRpc: string;
  consensusRpc: string;
  network: 'mainnet' | 'holesky' | 'sepolia';
  checkpoint?: string;
  minConfirmationsForSafety?: number;
  maxSnapshotStalenessBlocks?: number;
}

export class HeliosVerifier {
  private provider: HeliosProvider | null = null;
  private config: Config;
  private networkKind: NetworkKind;
  private minConfirmations: number;
  private maxStaleness: number;

  constructor(config: VerifierConfig) {
    this.config = {
      executionRpc: config.executionRpc,
      consensusRpc: config.consensusRpc,
      network: config.network as Network,
      checkpoint: config.checkpoint,
    };
    this.networkKind = 'ethereum';
    this.minConfirmations = config.minConfirmationsForSafety ?? DEFAULT_MIN_CONFIRMATIONS;
    this.maxStaleness = config.maxSnapshotStalenessBlocks ?? DEFAULT_MAX_STALENESS_BLOCKS;
  }

  async init(): Promise<void> {
    this.provider = await createHeliosProvider(this.config, this.networkKind);
    await this.provider.waitSynced();
  }

  async getBlockHash(blockNumber: bigint): Promise<string> {
    if (!this.provider) throw new Error('Not initialized');

    const block = await this.provider.request({
      method: 'eth_getBlockByNumber',
      params: [`0x${blockNumber.toString(16)}`, false],
    }) as { hash: string } | null;

    if (!block) {
      throw new Error(`Block ${blockNumber} not found`);
    }

    return block.hash;
  }

  async getCurrentBlock(): Promise<bigint> {
    if (!this.provider) throw new Error('Not initialized');

    const result = await this.provider.request({
      method: 'eth_blockNumber',
      params: [],
    }) as string;

    return BigInt(result);
  }

  async getFinalizedBlock(): Promise<bigint | undefined> {
    if (!this.provider) throw new Error('Not initialized');

    try {
      const block = await this.provider.request({
        method: 'eth_getBlockByNumber',
        params: ['finalized', false],
      }) as { number: string } | null;

      if (block) {
        return BigInt(block.number);
      }
    } catch {
      return undefined;
    }

    return undefined;
  }

  verifySnapshotEffect(
    expectedBlockNumber: bigint,
    expectedBlockHash: string
  ): Effect.Effect<
    VerificationResult,
    HashMismatchError | SnapshotInFutureError | TooRecentError | TooStaleError | NotFinalizedError | HeliosError
  > {
    type VerifyError = HashMismatchError | SnapshotInFutureError | TooRecentError | TooStaleError | NotFinalizedError | HeliosError;

    return Effect.gen(this, function* () {
      const { heliosBlockHash, currentBlock, finalizedBlock } = yield* Effect.tryPromise({
        try: async () => {
          const [hash, current, finalized] = await Promise.all([
            this.getBlockHash(expectedBlockNumber),
            this.getCurrentBlock(),
            this.getFinalizedBlock(),
          ]);
          return { heliosBlockHash: hash, currentBlock: current, finalizedBlock: finalized };
        },
        catch: (error): VerifyError => new HeliosError({ cause: error }),
      });

      const normalizedExpected = expectedBlockHash.toLowerCase();
      const normalizedHelios = heliosBlockHash.toLowerCase();
      const hashMatch = normalizedExpected === normalizedHelios;
      const depth = currentBlock - expectedBlockNumber;

      if (!hashMatch) {
        return yield* Effect.fail<VerifyError>(new HashMismatchError({
          expected: normalizedExpected,
          actual: normalizedHelios,
          blockNumber: expectedBlockNumber,
        }));
      }

      if (expectedBlockNumber > currentBlock) {
        return yield* Effect.fail<VerifyError>(new SnapshotInFutureError({
          snapshotBlock: expectedBlockNumber,
          currentBlock,
        }));
      }

      if (depth < BigInt(this.minConfirmations)) {
        return yield* Effect.fail<VerifyError>(new TooRecentError({
          depth,
          minRequired: this.minConfirmations,
        }));
      }

      if (depth > BigInt(this.maxStaleness)) {
        return yield* Effect.fail<VerifyError>(new TooStaleError({
          depth,
          maxAllowed: this.maxStaleness,
        }));
      }

      if (finalizedBlock !== undefined && expectedBlockNumber > finalizedBlock) {
        return yield* Effect.fail<VerifyError>(new NotFinalizedError({
          snapshotBlock: expectedBlockNumber,
          finalizedBlock,
        }));
      }

      const result: VerificationResult = {
        valid: true,
        verified: true,
        heliosBlockHash: normalizedHelios,
        expectedBlockHash: normalizedExpected,
        blockNumber: expectedBlockNumber,
        latestBlock: currentBlock,
        finalizedBlock,
        depthFromHead: depth,
        status: [],
      };

      return result;
    });
  }

  async verifySnapshotBlock(
    expectedBlockNumber: bigint,
    expectedBlockHash: string
  ): Promise<VerificationResult> {
    const result = await Effect.runPromise(
      pipe(
        this.verifySnapshotEffect(expectedBlockNumber, expectedBlockHash),
        Effect.catchAll((error) => {
          const statuses: VerificationStatus[] = [];
          let errorMessage: string | undefined;

          if (error._tag === 'HashMismatchError') {
            statuses.push('hash_mismatch');
            errorMessage = `Hash mismatch: expected ${error.expected}, got ${error.actual}`;
          } else if (error._tag === 'SnapshotInFutureError') {
            statuses.push('snapshot_in_future');
            errorMessage = `Snapshot block ${error.snapshotBlock} is in the future (current: ${error.currentBlock})`;
          } else if (error._tag === 'TooRecentError') {
            statuses.push('too_recent_reorg_risk');
            errorMessage = `Snapshot depth ${error.depth} < min required ${error.minRequired}`;
          } else if (error._tag === 'TooStaleError') {
            statuses.push('too_stale');
            errorMessage = `Snapshot depth ${error.depth} > max allowed ${error.maxAllowed}`;
          } else if (error._tag === 'NotFinalizedError') {
            statuses.push('not_finalized');
            errorMessage = `Snapshot block ${error.snapshotBlock} > finalized block ${error.finalizedBlock}`;
          } else if (error._tag === 'HeliosError') {
            statuses.push('helios_error');
            errorMessage = String(error.cause);
          }

          const failedResult: VerificationResult = {
            valid: error._tag !== 'HashMismatchError',
            verified: false,
            heliosBlockHash: '',
            expectedBlockHash: expectedBlockHash.toLowerCase(),
            blockNumber: expectedBlockNumber,
            status: statuses,
            error: errorMessage,
          };

          return Effect.succeed(failedResult);
        })
      )
    );

    return result;
  }
}
