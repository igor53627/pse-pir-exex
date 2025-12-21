import { describe, it, expect } from 'vitest';
import {
  HashMismatchError,
  SnapshotInFutureError,
  TooRecentError,
  TooStaleError,
  NotFinalizedError,
  ChainIdMismatchError,
  NETWORK_CHAIN_IDS,
  DEFAULT_MIN_CONFIRMATIONS,
  DEFAULT_MAX_STALENESS_BLOCKS,
} from './types.js';

describe('Error Types', () => {
  it('should create HashMismatchError with correct properties', () => {
    const error = new HashMismatchError({
      expected: '0xabc',
      actual: '0xdef',
      blockNumber: 100n,
    });

    expect(error._tag).toBe('HashMismatchError');
    expect(error.expected).toBe('0xabc');
    expect(error.actual).toBe('0xdef');
    expect(error.blockNumber).toBe(100n);
  });

  it('should create SnapshotInFutureError with correct properties', () => {
    const error = new SnapshotInFutureError({
      snapshotBlock: 200n,
      currentBlock: 100n,
    });

    expect(error._tag).toBe('SnapshotInFutureError');
    expect(error.snapshotBlock).toBe(200n);
    expect(error.currentBlock).toBe(100n);
  });

  it('should create TooRecentError with correct properties', () => {
    const error = new TooRecentError({
      depth: 10n,
      minRequired: 64,
    });

    expect(error._tag).toBe('TooRecentError');
    expect(error.depth).toBe(10n);
    expect(error.minRequired).toBe(64);
  });

  it('should create TooStaleError with correct properties', () => {
    const error = new TooStaleError({
      depth: 1000n,
      maxAllowed: 900,
    });

    expect(error._tag).toBe('TooStaleError');
    expect(error.depth).toBe(1000n);
    expect(error.maxAllowed).toBe(900);
  });

  it('should create NotFinalizedError with correct properties', () => {
    const error = new NotFinalizedError({
      snapshotBlock: 150n,
      finalizedBlock: 100n,
    });

    expect(error._tag).toBe('NotFinalizedError');
    expect(error.snapshotBlock).toBe(150n);
    expect(error.finalizedBlock).toBe(100n);
  });

  it('should create ChainIdMismatchError with correct properties', () => {
    const error = new ChainIdMismatchError({
      expected: 1,
      actual: 11155111,
    });

    expect(error._tag).toBe('ChainIdMismatchError');
    expect(error.expected).toBe(1);
    expect(error.actual).toBe(11155111);
  });
});

describe('Constants', () => {
  it('should have correct network chain IDs', () => {
    expect(NETWORK_CHAIN_IDS.mainnet).toBe(1);
    expect(NETWORK_CHAIN_IDS.holesky).toBe(17000);
    expect(NETWORK_CHAIN_IDS.sepolia).toBe(11155111);
  });

  it('should have correct default values', () => {
    expect(DEFAULT_MIN_CONFIRMATIONS).toBe(64);
    expect(DEFAULT_MAX_STALENESS_BLOCKS).toBe(900);
  });
});
