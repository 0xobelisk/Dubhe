# Sovereign SDK MetaMask Snap

The Sovereign SDK MetaMask Snap enables transaction signing for users.

This Snap is configured to use the
designated [coin type 1551](https://github.com/satoshilabs/slips/blob/master/slip-0044.md#registered-coin-types),
allocated for the SDK-Sovereign SDK. In accordance with Metamask's Snap permissions design, the coin type is hardcoded.
If you require a different coin type, you can fork this repository and modify the authorized path in
the [Snap manifest](./packages/snap/snap.md).

## Methods

#### `getAddress`

Returns the address of the wallet for the configured Sovereign SDK chain. The address is derived from a ed25519 public
key as a bech32m encoded hexadecimal string, prefixed with an identifier specific for the configured chain.

##### Params

- `keyId`: The key id used in the BIP-32 derivation path of the wallet. For example, a key id `0` (as a number) results
  in a derivation path of `string['m', "44'", "1551'", "0'"]`.

##### Example

```typescript
const response = await request({
    method: 'getAddress',
    params: {
        keyId: 0,
    },
});
if (
    !response ===
    'sov1dpvv2cvyv3cfsflx4hz67gyqcm3xlrj5v2ldyagep2uf8fwt7drsv936j0'
) {
    throw new Error('Invalid public key');
}
```

#### `signTransaction`

Returns the signature of the message as hexadecimal string.

Will emit a confirmation dialog for the user.

##### Params

- `keyId`: The key id used in the BIP-32 derivation path of the wallet. For example, a key id `0` (as a number) results
  in a derivation path of `string['m', "44'", "1551'", "0'"]`.
- `transaction`: A transaction to be serialized according to the configured Sovereign SDK rollup. The signature will be
  performed over the serialized transaction.

##### Example

```typescript
const response = request({
    method: 'signTransaction',
    params: {
        keyId: 0,
        transaction: {
            "call": '{"bank":{"CreateToken":{"salt":11,"token_name":"sov-test-token","initial_balance":1000000,"minter_address":"sov15vspj48hpttzyvxu8kzq5klhvaczcpyxn6z6k0hwpwtzs4a6wkvqwr57gc","authorized_minters":["sov1l6n2cku82yfqld30lanm2nfw43n2auc8clw7r5u5m6s7p8jrm4zqrr8r94","sov15vspj48hpttzyvxu8kzq5klhvaczcpyxn6z6k0hwpwtzs4a6wkvqwr57gc"]}}}',
            "nonce": 0,
            "chain_id": 4321,
            "max_priority_fee_bips": 0,
            "max_fee": 10000,
            "gas_limit": []
        },
    },
});

if (
    !response ===
    '0xfd2e4b23a3e3f498664af355b341e833324276270a13f9647dd1f043248f92fccaa037d4cfc9d23f13a295f7d505ee13afb2b10cea548890678f9002947cbb0a'
) {
    throw new Error('Invalid signature');
}
```

## Testing

To test the snap, run `yarn test` in this directory. This will
use [`@metamask/snaps-jest`](https://github.com/MetaMask/snaps/tree/main/packages/snaps-jest) to run the tests
in `src/index.test.ts`.

If a change to the snap code was performed, you will need to run `yarn build` before the tests.
