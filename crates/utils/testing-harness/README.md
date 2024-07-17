# Testing harness

This crate contains binary that can execute a set of transactions against [sov-demo-rollup](../../examples/demo-rollup/README_CELESTIA.md) on Celestia DA.


### Example command:

Make sure the celestia version of the demo rollup is running in another terminal.

Then, from the repo root, to run:

```bash
cargo run --bin testing-harness -- \
    --private-keys-dir examples/test-data/keys \
    --genesis-dir examples/test-data/genesis/demo/celestia \
    --rollup-config-path examples/demo-rollup/demo_rollup_config.toml \
    --max-batch-size-tx=5 \
    --max-batch-size-bytes=10000 \
    --new-users-count=10 \
    --max-num-txs=50 \
    --interval=2000
```

__Note:__ If you omit `--max-num-txs` the harness will run continuously.
__Note:__ If you omit `--interval` the harness will submit transactions as fast as possible whilst adhering to the configured `--max-batch-size-txs`.

### Current limitations

 * Only `sov-demo-rollup` runtime
 * Only Celestia DA
 * Only single sequencer, and binary submits batches directly to DA
 * Only checks that all transactions have been succeeded not change in state
 * Interval is not granular at the module level

### To Do
:black_square_button: Add something to the module message generators that can be used to query the expected state change after successful tx broadcasting