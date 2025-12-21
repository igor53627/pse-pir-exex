import { Effect } from 'effect';
import { createHeliosProvider } from '@a16z/helios';
import { HashMismatchError, SnapshotInFutureError, TooRecentError, TooStaleError, NotFinalizedError, HeliosError, DEFAULT_MIN_CONFIRMATIONS, DEFAULT_MAX_STALENESS_BLOCKS, } from './types.js';
export class HeliosVerifier {
    provider = null;
    config;
    networkKind;
    minConfirmations;
    maxStaleness;
    constructor(config) {
        this.config = {
            executionRpc: config.executionRpc,
            consensusRpc: config.consensusRpc,
            network: config.network,
            checkpoint: config.checkpoint,
        };
        this.networkKind = 'ethereum';
        this.minConfirmations = config.minConfirmationsForSafety ?? DEFAULT_MIN_CONFIRMATIONS;
        this.maxStaleness = config.maxSnapshotStalenessBlocks ?? DEFAULT_MAX_STALENESS_BLOCKS;
    }
    async init() {
        this.provider = await createHeliosProvider(this.config, this.networkKind);
        await this.provider.waitSynced();
    }
    async getBlockHash(blockNumber) {
        if (!this.provider)
            throw new Error('Not initialized');
        const block = await this.provider.request({
            method: 'eth_getBlockByNumber',
            params: [`0x${blockNumber.toString(16)}`, false],
        });
        if (!block) {
            throw new Error(`Block ${blockNumber} not found`);
        }
        return block.hash;
    }
    async getCurrentBlock() {
        if (!this.provider)
            throw new Error('Not initialized');
        const result = await this.provider.request({
            method: 'eth_blockNumber',
            params: [],
        });
        return BigInt(result);
    }
    async getFinalizedBlock() {
        if (!this.provider)
            throw new Error('Not initialized');
        try {
            const block = await this.provider.request({
                method: 'eth_getBlockByNumber',
                params: ['finalized', false],
            });
            if (block) {
                return BigInt(block.number);
            }
        }
        catch {
            return undefined;
        }
        return undefined;
    }
    verifySnapshotEffect(expectedBlockNumber, expectedBlockHash) {
        const self = this;
        return Effect.gen(function* () {
            const { currentBlock, finalizedBlock } = yield* Effect.tryPromise({
                try: async () => {
                    const [current, finalized] = await Promise.all([
                        self.getCurrentBlock(),
                        self.getFinalizedBlock(),
                    ]);
                    return { currentBlock: current, finalizedBlock: finalized };
                },
                catch: (error) => new HeliosError({ cause: error }),
            });
            if (expectedBlockNumber > currentBlock) {
                return yield* Effect.fail(new SnapshotInFutureError({
                    snapshotBlock: expectedBlockNumber,
                    currentBlock,
                }));
            }
            const heliosBlockHash = yield* Effect.tryPromise({
                try: () => self.getBlockHash(expectedBlockNumber),
                catch: (error) => new HeliosError({ cause: error }),
            });
            const normalizedExpected = expectedBlockHash.toLowerCase();
            const normalizedHelios = heliosBlockHash.toLowerCase();
            const hashMatch = normalizedExpected === normalizedHelios;
            const depth = currentBlock - expectedBlockNumber;
            if (!hashMatch) {
                return yield* Effect.fail(new HashMismatchError({
                    expected: normalizedExpected,
                    actual: normalizedHelios,
                    blockNumber: expectedBlockNumber,
                }));
            }
            if (depth < BigInt(self.minConfirmations)) {
                return yield* Effect.fail(new TooRecentError({
                    depth,
                    minRequired: self.minConfirmations,
                }));
            }
            if (depth > BigInt(self.maxStaleness)) {
                return yield* Effect.fail(new TooStaleError({
                    depth,
                    maxAllowed: self.maxStaleness,
                }));
            }
            if (finalizedBlock !== undefined && expectedBlockNumber > finalizedBlock) {
                return yield* Effect.fail(new NotFinalizedError({
                    snapshotBlock: expectedBlockNumber,
                    finalizedBlock,
                }));
            }
            const result = {
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
    async verifySnapshotBlock(expectedBlockNumber, expectedBlockHash) {
        const result = await Effect.runPromise(Effect.catchAll(this.verifySnapshotEffect(expectedBlockNumber, expectedBlockHash), (error) => {
            const statuses = [];
            let errorMessage;
            let actualHash = '';
            if (error._tag === 'HashMismatchError') {
                statuses.push('hash_mismatch');
                errorMessage = `Hash mismatch: expected ${error.expected}, got ${error.actual}`;
                actualHash = error.actual;
            }
            else if (error._tag === 'SnapshotInFutureError') {
                statuses.push('snapshot_in_future');
                errorMessage = `Snapshot block ${error.snapshotBlock} is in the future (current: ${error.currentBlock})`;
            }
            else if (error._tag === 'TooRecentError') {
                statuses.push('too_recent_reorg_risk');
                errorMessage = `Snapshot depth ${error.depth} < min required ${error.minRequired}`;
            }
            else if (error._tag === 'TooStaleError') {
                statuses.push('too_stale');
                errorMessage = `Snapshot depth ${error.depth} > max allowed ${error.maxAllowed}`;
            }
            else if (error._tag === 'NotFinalizedError') {
                statuses.push('not_finalized');
                errorMessage = `Snapshot block ${error.snapshotBlock} > finalized block ${error.finalizedBlock}`;
            }
            else if (error._tag === 'HeliosError') {
                statuses.push('helios_error');
                errorMessage = String(error.cause);
            }
            const failedResult = {
                valid: error._tag !== 'HashMismatchError' && error._tag !== 'HeliosError',
                verified: false,
                heliosBlockHash: actualHash,
                expectedBlockHash: expectedBlockHash.toLowerCase(),
                blockNumber: expectedBlockNumber,
                status: statuses,
                error: errorMessage,
            };
            return Effect.succeed(failedResult);
        }));
        return result;
    }
}
//# sourceMappingURL=helios-verifier.js.map