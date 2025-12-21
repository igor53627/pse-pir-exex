import type { WalletCoreConfig, BalanceResult, VerificationResult } from './types.js';
import { PirBalanceClient } from './pir-client.js';
import { HeliosVerifier } from './helios-verifier.js';

export class WalletCore {
  private pirClient: PirBalanceClient;
  private heliosVerifier: HeliosVerifier;
  private config: WalletCoreConfig;
  private initialized = false;
  private snapshotVerified = false;

  constructor(config: WalletCoreConfig) {
    this.config = config;
    this.pirClient = new PirBalanceClient(config.pirServerUrl);
    this.heliosVerifier = new HeliosVerifier({
      executionRpc: config.executionRpc,
      consensusRpc: config.consensusRpc,
      network: config.network,
      checkpoint: config.checkpoint,
    });
  }

  async init(): Promise<void> {
    await Promise.all([
      this.pirClient.init(),
      this.heliosVerifier.init(),
    ]);
    this.initialized = true;
  }

  async verifySnapshot(): Promise<VerificationResult> {
    if (!this.initialized) throw new Error('Not initialized');

    const snapshotBlock = this.pirClient.getSnapshotBlock();
    const expectedHash = this.pirClient.getSnapshotBlockHash();

    const result = await this.heliosVerifier.verifySnapshotBlock(
      snapshotBlock,
      expectedHash
    );

    this.snapshotVerified = result.valid;
    return result;
  }

  async getBalance(address: string): Promise<BalanceResult> {
    if (!this.initialized) throw new Error('Not initialized');

    const balance = await this.pirClient.queryBalance(address);

    if (!balance) {
      return {
        address,
        ethBalance: 0n,
        usdcBalance: 0n,
        snapshotBlock: this.pirClient.getSnapshotBlock(),
        verified: this.snapshotVerified,
      };
    }

    return {
      address,
      ethBalance: balance.eth,
      usdcBalance: balance.usdc,
      snapshotBlock: this.pirClient.getSnapshotBlock(),
      verified: this.snapshotVerified,
    };
  }

  async getBalances(addresses: string[]): Promise<BalanceResult[]> {
    return Promise.all(addresses.map(addr => this.getBalance(addr)));
  }

  isAddressInHotLane(address: string): boolean {
    if (!this.initialized) throw new Error('Not initialized');
    return this.pirClient.findAddressIndex(address) >= 0;
  }

  getSnapshotInfo(): { block: bigint; hash: string; verified: boolean } {
    if (!this.initialized) throw new Error('Not initialized');
    return {
      block: this.pirClient.getSnapshotBlock(),
      hash: this.pirClient.getSnapshotBlockHash(),
      verified: this.snapshotVerified,
    };
  }

  async getCurrentBlock(): Promise<bigint> {
    return this.heliosVerifier.getCurrentBlock();
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
