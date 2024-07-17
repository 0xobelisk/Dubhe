# Demo Rollup ![Time - ~5 mins](https://img.shields.io/badge/Time-~5_mins-informational)

<p align="center">
  <img width="50%" src="../../docs/assets/discord-banner.png">
  <br>
  <i>Stuck, facing problems, or unsure about something?</i>
  <br>
  <i>Join our <a href="https://discord.gg/kbykCcPrcA">Discord</a> and ask your questions in <code>#support</code>!</i>
</p>

#### Table of Contents

<!-- https://github.com/thlorenz/doctoc -->
<!-- $ doctoc README.md --github --notitle -->
<!-- START doctoc generated TOC please keep comment here to allow auto update -->
<!-- DON'T EDIT THIS SECTION, INSTEAD RE-RUN doctoc TO UPDATE -->

- [What is This?](#what-is-this)
- [Getting Started](#getting-started)
  - [Run a local DA layer instance](#run-a-local-da-layer-instance)
  - [Start the Rollup Full Node](#start-the-rollup-full-node)
  - [Sanity Check: Creating a Token](#sanity-check-creating-a-token)
  - [How to Submit Transactions](#how-to-submit-transactions)
    - [1. Build `sov-cli`](#1-build-sov-cli)
    - [2. Generate the Transaction](#2-generate-the-transaction)
    - [3. Make sure all the accounts involved have enough funds to pay for the transaction.](#3-make-sure-all-the-accounts-involved-have-enough-funds-to-pay-for-the-transaction)
    - [4. Submit the Transaction(s)](#4-submit-the-transactions)
    - [5. Verify the Token Supply](#5-verify-the-token-supply)
- [Disclaimer](#disclaimer)
- [Interacting with your Node via REST API](#interacting-with-your-node-via-rest-api)
- [Testing with specific DA layers](#testing-with-specific-da-layers)
- [License](#license)

<!-- END doctoc generated TOC please keep comment here to allow auto update -->

## What is This?

This demo shows how to integrate a State Transition Function (STF) with a Data Availability (DA) layer and a zkVM to
create a full
zk-rollup. The code in this repository corresponds to running a full-node of the rollup, which executes
every transaction.

By swapping out or modifying the imported state transition function, you can customize
this example full-node to run arbitrary logic.
This particular example relies on the state transition exported by [`demo-stf`](../demo-rollup/stf/). If you want to
understand how to build your own state transition function, check out at the docs in that package.

## Getting Started

If you are looking for a simple rollup with minimal dependencies as a starting point, please have a look here:
[sov-rollup-starter](https://github.com/Sovereign-Labs/sov-rollup-starter/)

If you don't need ZK guest to be compiled, for faster compilation time you can export `export SKIP_GUEST_BUILD=1`
environment
variable in each terminal you run. By default, demo-rollup disables proving. If you want to enable proving, several options
are available:

- `export SOV_PROVER_MODE=skip` Skips verification logic.
- `export SOV_PROVER_MODE=simulate` Run the rollup verification logic inside the current process.
- `export SOV_PROVER_MODE=execute` Run the rollup verifier in a zkVM executor.
- `export SOV_PROVER_MODE=prove` Run the rollup verifier and create a SNARK of execution.

### Run a local DA layer instance

This setup works with an in-memory DA that is easy to set up for testing purposes.

### Start the Rollup Full Node

1. Switch to the `examples/demo-rollup` and compile the application:

```shell,test-ci
$ cd examples/demo-rollup/
$ make build
```

2. Clean up the existing database.
   Makefile to simplify that process:

```sh,test-ci
$ make clean
```

3. Now run the demo-rollup full node, as shown below.

```sh,test-ci
$ export SOV_PROVER_MODE=execute
```

```sh,test-ci,bashtestmd:long-running,bashtestmd:wait-until=rpc_address
$ cargo run
```

Leave it running while you proceed with the rest of the demo.

### Sanity Check: Creating a Token

After switching to a new terminal tab, let's submit our first transaction by creating a token:

```sh,test-ci
$ make test-create-token
```

Once a batch is submitted the output should also contain the transaction hashes that have been submitted. For example -

```text
Your batch was submitted to the sequencer for publication. Response: "Submitted 1 transactions"
0: 66d4a27dd46013f88c156d21d16d364f6a5de66effd74155a5b0815475cbdf17
```

The transaction hash can be used to query the RPC endpoint to fetch events belonging to the transaction, which should in
this case have the TokenCreated Event

```sh,test-ci
$ curl -sS http://127.0.0.1:12346/ledger/txs/0x950cb82721602fcbbd5c4fb9950aa6c718f0140b3112789d370ebc707b6d6c01/events | jq
{
  "data": [
    {
      "type": "event",
      "number": 0,
      "key": "token_created",
      "value": {
        "TokenCreated": {
          "token_name": "sov-test-token",
          "coins": {
            "amount": 1000000,
            "token_id": "token_1zdwj8thgev2u3yyrrlekmvtsz4av4tp3m7dm5mx5peejnesga27ss0lusz"
          },
          "minter": {
            "User": "sov15vspj48hpttzyvxu8kzq5klhvaczcpyxn6z6k0hwpwtzs4a6wkvqwr57gc"
          },
          "authorized_minters": [
            {
              "User": "sov1l6n2cku82yfqld30lanm2nfw43n2auc8clw7r5u5m6s7p8jrm4zqrr8r94"
            },
            {
              "User": "sov15vspj48hpttzyvxu8kzq5klhvaczcpyxn6z6k0hwpwtzs4a6wkvqwr57gc"
            }
          ]
        }
      },
      "module": {
        "type": "moduleRef",
        "name": "bank"
      }
    }
  ],
  "meta": {}
}
```

We can see the TokenCreated event which contains the id of the token
created - `token_1rwrh8gn2py0dl4vv65twgctmlwck6esm2as9dftumcw89kqqn3nqrduss6`

### How to Submit Transactions

The `make test-create-token` command above was useful to test if everything is running correctly. Now let's get a better
understanding of how to create and submit a transaction.

#### 1. Build `sov-cli`

You'll need the `sov-cli` binary in order to create transactions. Build it with these commands:

```bash,test-ci,bashtestmd:compare-output
# Make sure you're still in `examples/demo-rollup`
$ SKIP_GUEST_BUILD=1 cargo build --bin sov-cli
$ ./../../target/debug/sov-cli --help
Usage: sov-cli <COMMAND>

Commands:
  transactions  Generate, sign, list and remove transactions
  keys          View and manage keys associated with this wallet
  rpc           Query the current state of the rollup and send transactions
  help          Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help
  -V, --version  Print version
```

Each transaction that we want to submit is a member of the `CallMessage` enum defined as part of creating a module. For
example, let's consider the `Bank` module's `CallMessage`:

```rust
use sov_bank::CallMessage::Transfer;
use sov_bank::Coins;
use sov_bank::TokenId;
use sov_bank::Amount;

pub enum CallMessage<S: sov_modules_api::Spec> {
    /// Creates a new token with the specified name and initial balance.
    CreateToken {
        /// Random value used to create a unique token ID.
        salt: u64,
        /// The name of the new token.
        token_name: String,
        /// The initial balance of the new token.
        initial_balance: Amount,
        /// The address of the account that the new tokens are minted to.
        mint_to_address: S::Address,
        /// Authorized minter list.
        authorized_minters: Vec<S::Address>,
    },

    /// Transfers a specified amount of tokens to the specified address.
    Transfer {
        /// The address to which the tokens will be transferred.
        to: S::Address,
        /// The amount of tokens to transfer.
        coins: Coins,
    },

    /// Burns a specified amount of tokens.
    Burn {
        /// The amount of tokens to burn.
        coins: Coins,
    },

    /// Mints a specified amount of tokens.
    Mint {
        /// The amount of tokens to mint.
        coins: Coins,
        /// Address to mint tokens to
        mint_to_address: S::Address,
    },

    /// Freeze a token so that the supply is frozen
    Freeze {
        /// Address of the token to be frozen
        token_id: TokenId,
    },
}
```

In the above snippet, we can see that `CallMessage` in `Bank` supports five different types of calls. The `sov-cli` has
the ability to parse a JSON file that aligns with any of these calls and subsequently serialize them. The structure of
the JSON file, which represents the call, closely mirrors that of the Enum member. You can view the relevant JSON Schema
for `Bank` [here](../../module-system/module-schemas/schemas/sov-bank.json) Consider the `Transfer` message as an
example:

```rust
use sov_bank::Coins;

struct Transfer<S: sov_modules_api::Spec> {
    /// The address to which the tokens will be transferred.
    to: S::Address,
    /// The amount of tokens to transfer.
    coins: Coins,
}
```

Here's an example of a JSON representing the above call:

```json
{
  "Transfer": {
    "to": "sov1zgfpyysjzgfpyysjzgfpyysjzgfpyysjzgfpyysjzgfpyysjzgfqve8h6h",
    "coins": {
      "amount": 200,
      "token_id": "token_1rwrh8gn2py0dl4vv65twgctmlwck6esm2as9dftumcw89kqqn3nqrduss6"
    }
  }
}
```

#### 2. Generate the Transaction

The JSON above is the contents of the
file [`examples/test-data/requests/transfer.json`](../../examples/test-data/requests/transfer.json). We'll use this
transaction as our example for the rest of the tutorial. In order to send the transaction, we need to perform 2
operations:

- Import the transaction data into the wallet
- Sign and submit the transaction

Note: we're able to make a `Transfer` call here because we already created the token as part of the sanity check above,
using `make test-create-token`.

To generate transactions you can use the `transactions import from-file` subcommand, as shown below:

```bash,test-ci,bashtestmd:compare-output
$ ./../../target/debug/sov-cli transactions import from-file -h
Import a transaction from a JSON file at the provided path

Usage: sov-cli transactions import from-file <COMMAND>

Commands:
  bank                A subcommand for the `bank` module
  sequencer-registry  A subcommand for the `sequencer_registry` module
  value-setter        A subcommand for the `value_setter` module
  prover-incentives   A subcommand for the `prover_incentives` module
  accounts            A subcommand for the `accounts` module
  nonces              A subcommand for the `nonces` module
  nft                 A subcommand for the `nft` module
  help                Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

Let's go ahead and import the transaction into the wallet

```bash,test-ci,bashtestmd:compare-output
$ ./../../target/debug/sov-cli transactions import from-file bank --max-fee 100000000 --path ../test-data/requests/transfer.json
Adding the following transaction to batch:
{
  "tx": {
    "bank": {
      "Transfer": {
        "to": "sov1l6n2cku82yfqld30lanm2nfw43n2auc8clw7r5u5m6s7p8jrm4zqrr8r94",
        "coins": {
          "amount": 200,
          "token_id": "token_1rwrh8gn2py0dl4vv65twgctmlwck6esm2as9dftumcw89kqqn3nqrduss6"
        }
      }
    }
  },
  "details": {
    "max_priority_fee_bips": 0,
    "max_fee": 100000000,
    "gas_limit": null,
    "chain_id": 4321
  }
}
```

#### 3. Make sure all the accounts involved have enough funds to pay for the transaction.

For the transaction to be processed successfully, you have to ensure that the sender account has enough funds to pay for the transaction fees and the sequencer has staked enough tokens to pay for the pre-execution checks. This `README` file uses addresses from the `examples/test-data/genesis/demo/mock` folder, which are pre-populated with enough funds. 

To be able to execute most simple transactions, the transaction sender should have about `1_000_000_000` tokens on their account and the sequencer should have staked `100_000_000` tokens in the registry.

More details can be found in the Sovereign book [available here](https://github.com/Sovereign-Labs/sovereign-book).


#### 4. Submit the Transaction(s)

You now have a batch with a single transaction in your wallet. If you want to submit any more transactions as part of
this
batch, you can import them now. Finally, let's submit your transaction to the rollup.

```bash,test-ci
$ sleep 20  # Wait a bit for the `make test-create-token` transaction to be processed.
$ ./../../target/debug/sov-cli rpc submit-batch by-address sov1l6n2cku82yfqld30lanm2nfw43n2auc8clw7r5u5m6s7p8jrm4zqrr8r94
```

#### 5. Verify the Token Supply

```bash,test-ci,bashtestmd:compare-output
$ sleep 20  # Wait a bit for the block to be processed by the node
$ curl -sS -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","method":"bank_supplyOf","params":{"token_id":"token_1zdwj8thgev2u3yyrrlekmvtsz4av4tp3m7dm5mx5peejnesga27ss0lusz"},"id":1}' http://127.0.0.1:12345
{"jsonrpc":"2.0","result":{"amount":1000000},"id":1}
```

```bash,test-ci,bashtestmd:compare-output
$ curl -sS http://127.0.0.1:12346/ledger/aggregated-proofs/latest | jq 'if .data.publicData.initialSlotNumber >= 1 then true else false end'
true
```

## Disclaimer

> ⚠️ Warning! ⚠️

`demo-rollup` is a prototype! It contains known vulnerabilities and should not be used in production under any
circumstances.

## Interacting with your Node via REST API

By default, this implementation prints the state root and the number of blobs processed for each slot. To access any
other data, you'll
want to use our REST API server. You can configure its host and port in `rollup_config.toml`.

You can get an overview of all available endpoints by reading the OpenAPI specification [here](../../full-node//sov-ledger-apis//openapi-v3.yaml). Here's just a few example queries:

- `http://localhost:12346/ledger/events/17`
- `http://localhost:12346/ledger/txs/50/events/0`
- `http://localhost:12346/ledger/txs/0/events?key=base64key`
- `http://localhost:12346/ledger/batches/10/txs/2/events/0`

## Testing with specific DA layers

Check [here](./README_CELESTIA.md) if you want to run with dockerized local Celestia instance.

## License

Licensed under the [Apache License, Version 2.0](../../LICENSE).

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this repository by you, as defined in the Apache-2.0 license, shall be
licensed as above, without any additional terms or conditions.
