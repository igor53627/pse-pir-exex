import { Effect, pipe } from 'effect';
import { ChainIdMismatchError, PirQueryError, AddressNotFoundError, NotInitializedError, SnapshotNotVerifiedError, NETWORK_CHAIN_IDS, DEFAULT_MIN_CONFIRMATIONS, DEFAULT_MAX_STALENESS_BLOCKS, DEFAULT_REQUIRE_VERIFIED, } from './types.js';
import { PirBalanceClient } from './pir-client.js';
import { HeliosVerifier } from './helios-verifier.js';
export class WalletCore {
    pirClient;
    heliosVerifier;
    config;
    initialized = false;
    snapshotVerified = false;
    verificationResult = null;
    chainIdValidated = false;
    constructor(config) {
        this.config = config;
        this.pirClient = new PirBalanceClient(config.pirServerUrl);
        this.heliosVerifier = new HeliosVerifier({
            executionRpc: config.executionRpc,
            consensusRpc: config.consensusRpc,
            network: config.network,
            checkpoint: config.checkpoint,
            minConfirmationsForSafety: config.minConfirmationsForSafety ?? DEFAULT_MIN_CONFIRMATIONS,
            maxSnapshotStalenessBlocks: config.maxSnapshotStalenessBlocks ?? DEFAULT_MAX_STALENESS_BLOCKS,
        });
    }
    async init() {
        await Promise.all([
            this.pirClient.init(),
            this.heliosVerifier.init(),
        ]);
        this.initialized = true;
    }
    checkChainIdMismatch() {
        const metadata = this.pirClient.getMetadata();
        if (!metadata)
            return null;
        const expectedChainId = NETWORK_CHAIN_IDS[this.config.network];
        if (expectedChainId !== undefined && metadata.chainId !== expectedChainId) {
            return new ChainIdMismatchError({
                expected: expectedChainId,
                actual: metadata.chainId,
            });
        }
        return null;
    }
    async verifySnapshot() {
        if (!this.initialized)
            throw new Error('Not initialized');
        const chainIdError = this.checkChainIdMismatch();
        if (chainIdError) {
            const result = {
                valid: false,
                verified: false,
                heliosBlockHash: '',
                expectedBlockHash: this.pirClient.getSnapshotBlockHash(),
                blockNumber: this.pirClient.getSnapshotBlock(),
                status: ['chain_id_mismatch'],
                error: `Chain ID mismatch: expected ${chainIdError.expected}, got ${chainIdError.actual}`,
            };
            this.verificationResult = result;
            this.snapshotVerified = false;
            return result;
        }
        this.chainIdValidated = true;
        const snapshotBlock = this.pirClient.getSnapshotBlock();
        const expectedHash = this.pirClient.getSnapshotBlockHash();
        const result = await this.heliosVerifier.verifySnapshotBlock(snapshotBlock, expectedHash);
        this.verificationResult = result;
        this.snapshotVerified = result.verified;
        return result;
    }
    getBalanceEffect(address) {
        const self = this;
        return Effect.gen(function* () {
            if (!self.initialized) {
                return yield* Effect.fail(new NotInitializedError({}));
            }
            const requireVerified = self.config.requireVerifiedSnapshot ?? DEFAULT_REQUIRE_VERIFIED;
            if (requireVerified && !self.snapshotVerified) {
                return yield* Effect.fail(new SnapshotNotVerifiedError({}));
            }
            const balance = yield* Effect.tryPromise({
                try: () => self.pirClient.queryBalance(address),
                catch: (error) => new PirQueryError({
                    message: `PIR query failed: ${error}`,
                    cause: error,
                }),
            });
            if (!balance) {
                return yield* Effect.fail(new AddressNotFoundError({ address }));
            }
            return {
                address,
                ethBalance: balance.eth,
                usdcBalance: balance.usdc,
                snapshotBlock: self.pirClient.getSnapshotBlock(),
                verified: self.snapshotVerified,
                source: 'pir',
            };
        });
    }
    async getBalance(address) {
        if (!this.initialized) {
            throw new NotInitializedError({});
        }
        const requireVerified = this.config.requireVerifiedSnapshot ?? DEFAULT_REQUIRE_VERIFIED;
        if (requireVerified && !this.snapshotVerified) {
            throw new SnapshotNotVerifiedError({});
        }
        const balance = await this.pirClient.queryBalance(address);
        if (!balance) {
            throw new AddressNotFoundError({ address });
        }
        return {
            address,
            ethBalance: balance.eth,
            usdcBalance: balance.usdc,
            snapshotBlock: this.pirClient.getSnapshotBlock(),
            verified: this.snapshotVerified,
            source: 'pir',
        };
    }
    async getBalances(addresses) {
        return Promise.all(addresses.map(addr => this.getBalance(addr)));
    }
    async getBalanceWithFallback(address, rpcFallback) {
        if (!this.initialized)
            throw new Error('Not initialized');
        const isInHotLane = this.isAddressInHotLane(address);
        if (isInHotLane && this.snapshotVerified) {
            const balance = await this.pirClient.queryBalance(address);
            if (balance) {
                return {
                    address,
                    ethBalance: balance.eth,
                    usdcBalance: balance.usdc,
                    snapshotBlock: this.pirClient.getSnapshotBlock(),
                    verified: this.snapshotVerified,
                    source: 'pir',
                };
            }
        }
        const rpcBalance = await rpcFallback(address);
        return {
            address,
            ethBalance: rpcBalance.eth,
            usdcBalance: rpcBalance.usdc,
            snapshotBlock: 0n,
            verified: false,
            source: 'rpc',
        };
    }
    getBalanceWithFallbackEffect(address, rpcFallback) {
        const self = this;
        return pipe(this.getBalanceEffect(address), Effect.catchAll(() => Effect.tryPromise({
            try: async () => {
                const rpcBalance = await rpcFallback(address);
                return {
                    address,
                    ethBalance: rpcBalance.eth,
                    usdcBalance: rpcBalance.usdc,
                    snapshotBlock: 0n,
                    verified: false,
                    source: 'rpc',
                };
            },
            catch: (error) => new PirQueryError({
                message: `RPC fallback failed: ${error}`,
                cause: error,
            }),
        })));
    }
    isAddressInHotLane(address) {
        if (!this.initialized)
            throw new Error('Not initialized');
        return this.pirClient.findAddressIndex(address) >= 0;
    }
    getSnapshotInfo() {
        if (!this.initialized)
            throw new Error('Not initialized');
        return {
            block: this.pirClient.getSnapshotBlock(),
            hash: this.pirClient.getSnapshotBlockHash(),
            verified: this.snapshotVerified,
            chainId: this.pirClient.getMetadata()?.chainId,
            verificationResult: this.verificationResult ?? undefined,
        };
    }
    async getCurrentBlock() {
        return this.heliosVerifier.getCurrentBlock();
    }
    getVerificationStatus() {
        return this.verificationResult?.status ?? [];
    }
    isVerified() {
        return this.snapshotVerified;
    }
    dispose() {
        this.pirClient.dispose();
    }
}
export function formatEth(wei, decimals = 4) {
    const eth = Number(wei) / 1e18;
    return eth.toFixed(decimals);
}
export function formatUsdc(raw, decimals = 2) {
    const usdc = Number(raw) / 1e6;
    return usdc.toFixed(decimals);
}
//# sourceMappingURL=wallet-core.js.map