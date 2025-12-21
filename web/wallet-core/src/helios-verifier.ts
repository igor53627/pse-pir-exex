import type { VerificationResult } from './types.js';
import type { Config, NetworkKind, Network } from '@a16z/helios';
import { createHeliosProvider, HeliosProvider } from '@a16z/helios';

export class HeliosVerifier {
  private provider: HeliosProvider | null = null;
  private config: Config;
  private networkKind: NetworkKind;

  constructor(config: {
    executionRpc: string;
    consensusRpc: string;
    network: 'mainnet' | 'holesky' | 'sepolia';
    checkpoint?: string;
  }) {
    this.config = {
      executionRpc: config.executionRpc,
      consensusRpc: config.consensusRpc,
      network: config.network as Network,
      checkpoint: config.checkpoint,
    };
    this.networkKind = 'ethereum';
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

  async verifySnapshotBlock(
    expectedBlockNumber: bigint,
    expectedBlockHash: string
  ): Promise<VerificationResult> {
    const heliosBlockHash = await this.getBlockHash(expectedBlockNumber);

    const normalizedExpected = expectedBlockHash.toLowerCase();
    const normalizedHelios = heliosBlockHash.toLowerCase();

    return {
      valid: normalizedExpected === normalizedHelios,
      heliosBlockHash: normalizedHelios,
      expectedBlockHash: normalizedExpected,
      blockNumber: expectedBlockNumber,
    };
  }

  async getCurrentBlock(): Promise<bigint> {
    if (!this.provider) throw new Error('Not initialized');

    const result = await this.provider.request({
      method: 'eth_blockNumber',
      params: [],
    }) as string;

    return BigInt(result);
  }
}
