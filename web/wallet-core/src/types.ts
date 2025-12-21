export interface WalletCoreConfig {
  pirServerUrl: string;
  executionRpc: string;
  consensusRpc: string;
  network: 'mainnet' | 'holesky' | 'sepolia';
  checkpoint?: string;
}

export interface BalanceResult {
  address: string;
  ethBalance: bigint;
  usdcBalance: bigint;
  snapshotBlock: bigint;
  verified: boolean;
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

export interface VerificationResult {
  valid: boolean;
  heliosBlockHash: string;
  expectedBlockHash: string;
  blockNumber: bigint;
}

export const BALANCE_RECORD_SIZE = 64;
