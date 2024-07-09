import { SLIP10Node } from '@metamask/key-tree';
import { providerErrors, rpcErrors } from '@metamask/rpc-errors';
import type { OnRpcRequestHandler } from '@metamask/snaps-types';
import { DialogType } from '@metamask/snaps-types';
import { copyable, heading, panel, text } from '@metamask/snaps-ui';
import {add0x, assert, bytesToHex, hexToBytes, remove0x} from '@metamask/utils';
import { sign } from '@noble/ed25519';
import { validate as superstructValidate } from 'superstruct';

import {GetAddressStruct, SignTransactionStruct } from './types';
import { SovWasm } from './wasm';

const wasm = new SovWasm();

/**
 * Handle incoming JSON-RPC requests, sent through `wallet_invokeSnap`.
 *
 * @param args - The request handler args as object.
 * invoked the snap.
 * @param args.origin - The origin of the request.
 * @param args.request - A validated JSON-RPC request object.
 * @returns The result of `snap_dialog`.
 * @throws If the request method is not valid for this snap.
 */
export const onRpcRequest: OnRpcRequestHandler = async ({
  origin,
  request,
}) => {
  switch (request.method) {
    // the return is a plain hex string
    // https://docs.metamask.io/snaps/reference/rpc-api/#returns-5
    case 'getAddress': {
      console.trace('RPC Request: getAddress')
      const [validationErr, params] = superstructValidate(
        request.params,
        GetAddressStruct,
      );

      if (validationErr !== undefined) {
        console.error('Encountered validation error in getPublicKey:' + validationErr.toString());
        throw rpcErrors.invalidParams(validationErr.toString());
      }

      const { keyId } = params;
      console.log(`Got keyId ${keyId}`);
      const keypairParams = getKeypairParams(keyId);
      try {
        const publicKeyRequest = {
          method: 'snap_getBip32PublicKey',
          "params": keypairParams ,
        };
        console.trace('Requesting public key with args:' + JSON.stringify(publicKeyRequest));
        const publicKeyResponse = await snap.request(publicKeyRequest);
        const publicKey = cleanPublicKeyResponse(publicKeyResponse);

        console.trace('Bech encoding public key: {' + publicKey + "} to get address");
        const address = wasm.bechEncodePublicKey(publicKey);

        wasm.dealloc();
        console.trace('RPC Response: getAddress:'+ address);
        return address;
      } catch (er) {
        console.error('Error during requesting public key and encoding into address in getAddress:'+ er.toString());
        wasm.dealloc();
        throw er;
      }
    }

    case 'signTransaction': {
      console.trace('RPC Request: signTransaction')
      const [validationErr, params] = superstructValidate(
        request.params,
        SignTransactionStruct,
      );

      if (validationErr !== undefined) {
        console.error('Encountered validation error:' + validationErr.toString());
        throw rpcErrors.invalidParams(validationErr.toString());
      }

      let { transaction, keyId } = params;
      const keypairParams = getKeypairParams(keyId);
      try {
        const entropy = await snap.request({
          method: 'snap_getBip32Entropy',
          "params": keypairParams,
        });

        // we define a SLIP-10 node from the response
        // https://docs.metamask.io/snaps/reference/rpc-api/#returns-4
        const node = await SLIP10Node.fromJSON(entropy);
        assert(node.privateKey);

        const publicKey = cleanPublicKeyResponse(node.publicKey);
        console.trace('Bech encoding public key: {' + publicKey + "} to get address");
        const address = wasm.bechEncodePublicKey(publicKey);

        // eslint-disable-next-line @typescript-eslint/await-thenable
        const approved = await snap.request({
          method: 'snap_dialog',
          params: {
            type: DialogType.Confirmation,
            content: panel([
              heading('Signature request'),
              text(`The application at:`),
              copyable(origin),
              text(`is requesting a signature for the call message:`),
              copyable(JSON.stringify(transaction.call)),
              text(`from address:`),
              copyable(address),
              text(`with nonce:`),
              copyable(transaction.nonce.toString()),
              text(`with chainId:`),
              copyable(transaction.chain_id.toString()),
              text(`with maxPriorityFeeBips:`),
              copyable(transaction.max_priority_fee_bips.toString()),
              text(`with maxFee:`),
              copyable(transaction.max_fee.toString()),
              text(`with gasLimit:`),
              copyable(transaction.gas_limit ? transaction.gas_limit.join(', ') : 'Not Set'),
            ]),
          },
        });

        if (!approved) {
          console.error(`User rejected signature approval.` );
          throw providerErrors.userRejectedRequest();
        }

        console.trace("Serializing call message to get runtime message. Call message: " + JSON.stringify(transaction.call));
        const runtime_msg = Array.from(wasm.serializeCall(transaction.call));

        // Create a new unsigned transaction JSON object, by omitting "call" key from the transaction object,
        // copying the rest, and adding the serialized version of the call message, i.e. the runtime_msg
        const unsignedTransaction = {
          ...omitKey("call", transaction),
          runtime_msg
        }

        console.trace("Serializing unsigned transaction: " + JSON.stringify(unsignedTransaction));
        const serializedUnsignedTransaction = wasm.serializeUnsignedTransaction(unsignedTransaction);

        const privateKey = remove0x(node.privateKey);
        console.trace(`Signing unsigned transaction.\nSerializedUnsignedTransaction: ${serializedUnsignedTransaction}` );
        let sig = Array.from(await sign(serializedUnsignedTransaction, privateKey));

        // signature and pub_key fields and their peculiar format is AFAIU due to Risc0PublicKey and Risc0Signature
        // in our codebase. Transaction deserialization on the WASM front might break if you try to use a different
        // CryptoSpec::Signature and PublicKey.
        const signedTransaction = {
          "signature": {
            "msg_sig": sig
          },
          "pub_key": publicKey,
          ...unsignedTransaction
        };

        console.trace("Serializing signed transaction: " + JSON.stringify(signedTransaction));
        const tx = wasm.serializeTransaction(signedTransaction);
        const txHex = bytesToHex(tx);

        wasm.dealloc();
        console.trace('RPC Response: signTransaction: '+ txHex);
        return txHex;
      } catch (er) {
        wasm.dealloc();
        console.error('Error in signTransaction: '+ er.toString());
        throw er;
      }
    }

    default:
      throw new Error('Method not found.');
  }
};

const getKeypairParams = (keyId: number) => {
  return {
    "path": ['m', "44'", "1551'", `${keyId}'`],
    "compressed": true,
    "curve": "ed25519",
  }
}

const cleanPublicKeyResponse = (metamaskPublicKeyResponse: any) => {
  // Slicing is to skip the key byte flags prefix, which Metamask prepends the public key with.
  return remove0x(metamaskPublicKeyResponse).slice(2);
}

const omitKey = (key: string, obj: any) => {
  const { [key]: omitted, ...rest } = obj;
  return rest;
}
