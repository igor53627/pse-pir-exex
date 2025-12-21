import { Effect, pipe } from 'effect';

import type {
  WalletCoreConfig,
  BalanceResult,
  VerificationResult,
  VerificationStatus,
  BalanceError,
} from './types.js';
import {
  ChainIdMismatchError,
  PirQueryError,
  AddressNotFoundError,
  NETWORK_CHAIN_IDS,
  DEFAULT_MIN_CONFIRMATIONS,
  DEFAULT_MAX_STALENESS_BLOCKS,
  DEFAULT_REQUIRE_VERIFIED,
} from './types.js';
import { PirBalanceClient } from './pir-client.js';
import { HeliosVerifier } from './helios-verifier.js';

export class WalletCore {
  private pirClient: PirBalanceClient;
  private heliosVerifier: HeliosVerifier;
  private config: WalletCoreConfig;
  private initialized = false;
  private snapshotVerified = false;
  private verificationResult: VerificationResult | null = null;
  private chainIdValidated = false;

  constructor(config: WalletCoreConfig) {
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

  async init(): Promise<void> {
    await Promise.all([
      this.pirClient.init(),
      this.heliosVerifier.init(),
    ]);
    this.initialized = true;
  }

  validateChainId(): Effect.Effect<void, ChainIdMismatchError> {
    const metadata = this.pirClient.getMetadata();
    if (!metadata) {
      return Effect.void;
    }

    const expectedChainId = NETWORK_CHAIN_IDS[this.config.network];
    if (expectedChainId !== undefined && metadata.chainId !== expectedChainId) {
      return Effect.fail(new ChainIdMismatchError({
        expected: expectedChainId,
        actual: metadata.chainId,
      }));
    }

    return Effect.void;
  }

  async verifySnapshot(): Promise<VerificationResult> {
    if (!this.initialized) throw new Error('Not initialized');

    const chainIdResult = await Effect.runPromiseExit(this.validateChainId());
    if (chainIdResult._tag === 'Failure') {
      const error = chainIdResult.cause;
      const chainIdError = 'value' in error ? error.value : null;
      
      const result: VerificationResult = {
        valid: false,
        verified: false,
        heliosBlockHash: '',
        expectedBlockHash: this.pirClient.getSnapshotBlockHash(),
        blockNumber: this.pirClient.getSnapshotBlock(),
        status: ['chain_id_mismatch'] as VerificationStatus[],
        error: chainIdError 
          ? `Chain ID mismatch: expected ${(chainIdError as ChainIdMismatchError).expected}, got ${(chainIdError as ChainIdMismatchError).actual}`
          : 'Chain ID mismatch',
      };
      this.verificationResult = result;
      this.snapshotVerified = false;
      return result;
    }

    this.chainIdValidated = true;

    const snapshotBlock = this.pirClient.getSnapshotBlock();
    const expectedHash = this.pirClient.getSnapshotBlockHash();

    const result = await this.heliosVerifier.verifySnapshotBlock(
      snapshotBlock,
      expectedHash
    );

    this.verificationResult = result;
    this.snapshotVerified = result.verified;
    return result;
  }

  getBalanceEffect(
    address: string
  ): Effect.Effect<BalanceResult, BalanceError> {
    return pipe(
      Effect.sync(() => {
        if (!this.initialized) {
          throw new Error('Not initialized');
        }

        const requireVerified = this.config.requireVerifiedSnapshot ?? DEFAULT_REQUIRE_VERIFIED;
        if (requireVerified && !this.snapshotVerified) {
          throw new Error('Snapshot not verified; balances unavailable');
        }
      }),
      Effect.flatMap(() =>
        Effect.tryPromise({
          try: () => this.pirClient.queryBalance(address),
          catch: (error) => new PirQueryError({
            message: `PIR query failed: ${error}`,
            cause: error,
          }),
        })
      ),
      Effect.flatMap((balance) => {
        if (!balance) {
          return Effect.fail(new AddressNotFoundError({ address }));
        }

        return Effect.succeed({
          address,
          ethBalance: balance.eth,
          usdcBalance: balance.usdc,
          snapshotBlock: this.pirClient.getSnapshotBlock(),
          verified: this.snapshotVerified,
          source: 'pir' as const,
        });
      })
    );
  }

  async getBalance(address: string): Promise<BalanceResult> {
    if (!this.initialized) throw new Error('Not initialized');

    const requireVerified = this.config.requireVerifiedSnapshot ?? DEFAULT_REQUIRE_VERIFIED;
    if (requireVerified && !this.snapshotVerified) {
      throw new Error('Snapshot not verified; balances unavailable');
    }

    const balance = await this.pirClient.queryBalance(address);

    if (!balance) {
      return {
        address,
        ethBalance: 0n,
        usdcBalance: 0n,
        snapshotBlock: this.pirClient.getSnapshotBlock(),
        verified: this.snapshotVerified,
        source: 'pir',
      };
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

  async getBalances(addresses: string[]): Promise<BalanceResult[]> {
    return Promise.all(addresses.map(addr => this.getBalance(addr)));
  }

  async getBalanceWithFallback(
    address: string,
    rpcFallback: (address: string) => Promise<{ eth: bigint; usdc: bigint }>
  ): Promise<BalanceResult> {
    if (!this.initialized) throw new Error('Not initialized');

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

  getBalanceWithFallbackEffect(
    address: string,
    rpcFallback: (address: string) => Promise<{ eth: bigint; usdc: bigint }>
  ): Effect.Effect<BalanceResult, PirQueryError> {
    return pipe(
      this.getBalanceEffect(address),
      Effect.catchAll(() =>
        Effect.tryPromise({
          try: async () => {
            const rpcBalance = await rpcFallback(address);
            return {
              address,
              ethBalance: rpcBalance.eth,
              usdcBalance: rpcBalance.usdc,
              snapshotBlock: 0n,
              verified: false,
              source: 'rpc' as const,
            };
          },
          catch: (error) => new PirQueryError({
            message: `RPC fallback failed: ${error}`,
            cause: error,
          }),
        })
      )
    );
  }

  isAddressInHotLane(address: string): boolean {
    if (!this.initialized) throw new Error('Not initialized');
    return this.pirClient.findAddressIndex(address) >= 0;
  }

  getSnapshotInfo(): { 
    block: bigint; 
    hash: string; 
    verified: boolean;
    chainId?: number;
    verificationResult?: VerificationResult;
  } {
    if (!this.initialized) throw new Error('Not initialized');
    return {
      block: this.pirClient.getSnapshotBlock(),
      hash: this.pirClient.getSnapshotBlockHash(),
      verified: this.snapshotVerified,
      chainId: this.pirClient.getMetadata()?.chainId,
      verificationResult: this.verificationResult ?? undefined,
    };
  }

  async getCurrentBlock(): Promise<bigint> {
    return this.heliosVerifier.getCurrentBlock();
  }

  getVerificationStatus(): VerificationStatus[] {
    return this.verificationResult?.status ?? [];
  }

  isVerified(): boolean {
    return this.snapshotVerified;
  }

  dispose(): void {
    this.pirClient.dispose();
  }
}

export function formatEth(wei: bigint, decimals: number = 4): string {
  const eth = Number(wei) / 1e18;
  return eth.toFixed(decimals);
}

export function formatUsdc(raw: bigint, decimals: number = 2): string {
  const usdc = Number(raw) / 1e6;
  return usdc.toFixed(decimals);
}
