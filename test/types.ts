import { Bip32PathStruct } from '@metamask/snaps-utils';
import {
  boolean,
  object,
  optional,
  type,
  string,
  number,
  array,
} from 'superstruct';

/**
 * `type` is used instead of `object` to allow unknown properties.
 */
export const GetAddressStruct = type({
  /**
   * The key id of the account.
   */
  keyId: number(),
});

/**
 * The transaction object to be submitted by the UI so the signature can be generated.
 */
export const TransactionStruct = object({
  /**
   * The transaction info to sign.
   */
  call: object(),
  /**
   * The id for the chain that the transaction will be sent to.
   */
  chain_id: number(),
  /**
   * The maximum priority fee that can be paid for this transaction expressed
   * as a basis point percentage of the gas consumed by the transaction.
   */
  max_priority_fee_bips: number(),
  /**
   * The maximum fee that can be paid for this transaction expressed
   * as the gas token amount
   */
  max_fee: number(),
  /**
   * The gas limit for the transaction execution. This is an optional field that can be used to enforce a
   * gas limit on the transaction execution - in a way that reproduces the behavior of the EIP-1559. If the gas limit is
   * not provided, the transaction will be executed without checking the gas limit. The gas limit is a multi-dimensional gas vector
   * that specify the maximum amount of gas that can be used along each dimension.
   */
  gas_limit: optional(array(number())),
  /**
   * The nonce for the transaction signature.
   */
  nonce: number(),
});

/**
 * The parameters for calling the `signTransaction` JSON-RPC method.
 */
export const SignTransactionStruct = object({
  /**
   * The transaction info to sign.
   */
  transaction: TransactionStruct,

  /**
   * The key id of the account.
   * */
  keyId: number(),
});