import { Effect } from 'effect';
import type { WalletCoreConfig, BalanceResult, VerificationResult, VerificationStatus, BalanceError } from './types.js';
import { PirQueryError } from './types.js';
export declare class WalletCore {
    private pirClient;
    private heliosVerifier;
    private config;
    private initialized;
    private snapshotVerified;
    private verificationResult;
    private chainIdValidated;
    constructor(config: WalletCoreConfig);
    init(): Promise<void>;
    private checkChainIdMismatch;
    verifySnapshot(): Promise<VerificationResult>;
    getBalanceEffect(address: string): Effect.Effect<BalanceResult, BalanceError>;
    getBalance(address: string): Promise<BalanceResult>;
    getBalances(addresses: string[]): Promise<BalanceResult[]>;
    getBalanceWithFallback(address: string, rpcFallback: (address: string) => Promise<{
        eth: bigint;
        usdc: bigint;
    }>): Promise<BalanceResult>;
    getBalanceWithFallbackEffect(address: string, rpcFallback: (address: string) => Promise<{
        eth: bigint;
        usdc: bigint;
    }>): Effect.Effect<BalanceResult, PirQueryError>;
    isAddressInHotLane(address: string): boolean;
    getSnapshotInfo(): {
        block: bigint;
        hash: string;
        verified: boolean;
        chainId?: number;
        verificationResult?: VerificationResult;
    };
    getCurrentBlock(): Promise<bigint>;
    getVerificationStatus(): VerificationStatus[];
    isVerified(): boolean;
    dispose(): void;
}
export declare function formatEth(wei: bigint, decimals?: number): string;
export declare function formatUsdc(raw: bigint, decimals?: number): string;
//# sourceMappingURL=wallet-core.d.ts.map