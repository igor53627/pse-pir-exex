import type { BalanceMetadata } from './types.js';
export declare class PirBalanceClient {
    private client;
    private metadata;
    private serverUrl;
    private lane;
    constructor(serverUrl: string, lane?: string);
    init(): Promise<void>;
    getMetadata(): BalanceMetadata | null;
    getSnapshotBlock(): bigint;
    getSnapshotBlockHash(): string;
    findAddressIndex(address: string): number;
    queryBalance(address: string): Promise<{
        eth: bigint;
        usdc: bigint;
    } | null>;
    dispose(): void;
}
//# sourceMappingURL=pir-client.d.ts.map