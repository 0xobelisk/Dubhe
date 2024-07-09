import { Ed25519Keypair } from '@0xobelisk/sui-client';
import { bytesToHex } from '@metamask/utils';
import { SovWasm } from './wasm';

const wasm = new SovWasm();
const main = () => {
  const keypair = new Ed25519Keypair();
  const pk = bytesToHex(keypair.getPublicKey().toRawBytes());
  const address = wasm.bechEncodePublicKey(pk);
  console.log(address);
};
main();
