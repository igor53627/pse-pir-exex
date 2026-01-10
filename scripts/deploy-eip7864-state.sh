#!/bin/bash
# Deploy EIP-7864 state format to hsiao server
# Issue #72: Re-export Sepolia state with EIP-7864 tree_index
#
# This script:
# 1. Rebuilds all binaries from current commit
# 2. Backs up existing state data
# 3. Re-exports state with EIP-7864 tree_index
# 4. Regenerates stem index (expected: 7.5M -> ~60K stems)
# 5. Regenerates PIR database
# 6. Verifies results
#
# Usage:
#   ./scripts/deploy-eip7864-state.sh [--dry-run]
#
# Prerequisites:
#   - SSH access to hsiao (104.204.142.13)
#   - ethrex node running on hsiao with UBT sync completed

set -euo pipefail

# Configuration
REMOTE_HOST="root@104.204.142.13"
REMOTE_REPO="/root/inspire-exex"
DATA_DIR="/mnt/sepolia/pir-sync"
PIR_DATA_DIR="/mnt/sepolia/pir-data"
BACKUP_SUFFIX=".pre-eip7864-$(date +%Y%m%d-%H%M%S)"
DRY_RUN=false

# Parse args
if [[ "${1:-}" == "--dry-run" ]]; then
    DRY_RUN=true
    echo "[DRY-RUN] Would execute the following commands:"
    echo ""
fi

run_remote() {
    if $DRY_RUN; then
        echo "  ssh $REMOTE_HOST \"$1\""
    else
        ssh "$REMOTE_HOST" "$1"
    fi
}

echo "=== EIP-7864 State Deployment (Issue #72) ==="
echo "Remote host: $REMOTE_HOST"
echo "Data dir: $DATA_DIR"
echo "Backup suffix: $BACKUP_SUFFIX"
echo ""

# Step 1: Rebuild all binaries from same commit
echo "[1/6] Rebuilding binaries on hsiao..."
run_remote "cd $REMOTE_REPO && git pull && cargo build --release -p inspire-updater -p lane-builder"

BINARIES=(
    "inspire-updater"
    "stem-index"
    "state-to-pir"
)
echo "[OK] Built: ${BINARIES[*]}"
echo ""

# Step 2: Backup existing data
echo "[2/6] Backing up existing data..."
run_remote "
    if [[ -f $DATA_DIR/state.bin ]]; then
        mv $DATA_DIR/state.bin $DATA_DIR/state.bin$BACKUP_SUFFIX
        echo 'Backed up state.bin'
    fi
    if [[ -f $DATA_DIR/stem-index.bin ]]; then
        mv $DATA_DIR/stem-index.bin $DATA_DIR/stem-index.bin$BACKUP_SUFFIX
        echo 'Backed up stem-index.bin'
    fi
    if [[ -d $PIR_DATA_DIR ]]; then
        mv $PIR_DATA_DIR ${PIR_DATA_DIR}$BACKUP_SUFFIX
        echo 'Backed up pir-data'
    fi
"
echo "[OK] Backups created with suffix: $BACKUP_SUFFIX"
echo ""

# Step 3: Re-run state export with EIP-7864 format
echo "[3/6] Re-exporting state with EIP-7864 tree_index..."
run_remote "
    cd $REMOTE_REPO
    ./target/release/updater \
        --rpc http://localhost:8545 \
        --data-dir $DATA_DIR \
        --full-sync
"
echo "[OK] State export complete"
echo ""

# Step 4: Regenerate stem index with verification
echo "[4/6] Generating stem index with --verify..."
run_remote "
    cd $REMOTE_REPO
    ./target/release/stem-index \
        --input $DATA_DIR/state.bin \
        --output $DATA_DIR/stem-index.bin \
        --verify
"
echo "[OK] Stem index generated"
echo ""

# Step 5: Verify stem count (expected ~60K, not millions)
echo "[5/6] Verifying stem count..."
STEM_INDEX_SIZE=$(run_remote "stat -c%s $DATA_DIR/stem-index.bin")
STEM_COUNT=$(( (STEM_INDEX_SIZE - 8) / 39 ))
echo "Stem index size: $STEM_INDEX_SIZE bytes"
echo "Estimated stems: $STEM_COUNT"

if [[ $STEM_COUNT -gt 500000 ]]; then
    echo "[WARN] Stem count ($STEM_COUNT) is higher than expected (~60K)"
    echo "       This may indicate EIP-7864 co-location is not working as expected"
    echo "       Check if most storage slots are hashed mapping keys"
else
    echo "[OK] Stem count is in expected range (<500K)"
fi
echo ""

# Step 6: Regenerate PIR database
echo "[6/6] Regenerating PIR database..."
run_remote "
    cd $REMOTE_REPO
    mkdir -p $PIR_DATA_DIR
    ./target/release/state-to-pir \
        --input $DATA_DIR/state.bin \
        --output $PIR_DATA_DIR \
        --entry-size 84
"
echo "[OK] PIR database generated"
echo ""

# Summary
echo "=== Deployment Complete ==="
echo ""
echo "Results:"
run_remote "
    echo \"  state.bin:      \$(du -h $DATA_DIR/state.bin | cut -f1)\"
    echo \"  stem-index.bin: \$(du -h $DATA_DIR/stem-index.bin | cut -f1)\"
    echo \"  pir-data:       \$(du -sh $PIR_DATA_DIR | cut -f1)\"
"
echo ""
echo "Backups (to delete after verification):"
run_remote "ls -lh $DATA_DIR/*$BACKUP_SUFFIX 2>/dev/null || echo '  (none)'"
run_remote "ls -d ${PIR_DATA_DIR}$BACKUP_SUFFIX 2>/dev/null || echo '  (none)'"
echo ""
echo "Next steps:"
echo "  1. Verify stem count is ~60K (124x reduction from 7.5M)"
echo "  2. Restart inspire-server with new data"
echo "  3. Run end-to-end verification (compare eth_getStorageAt with PIR queries)"
echo "  4. Delete backups after confirming everything works"
echo ""
echo "To rollback:"
echo "  ssh $REMOTE_HOST"
echo "  mv $DATA_DIR/state.bin$BACKUP_SUFFIX $DATA_DIR/state.bin"
echo "  mv $DATA_DIR/stem-index.bin$BACKUP_SUFFIX $DATA_DIR/stem-index.bin"
echo "  mv ${PIR_DATA_DIR}$BACKUP_SUFFIX $PIR_DATA_DIR"
