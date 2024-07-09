import { rpcErrors } from '@metamask/rpc-errors';

import { moduleBytes } from './module';


type Argument = [number, number];
type InstanceMemorySlice = [number, number];
type WasmInstance = {
  alloc: (len: number) => number;
  dealloc: (ptr: number, len: number) => void;
  // eslint-disable-next-line @typescript-eslint/naming-convention
  bech_encode_public_key: (ptr: number, len: number) => number;
  // eslint-disable-next-line @typescript-eslint/naming-convention
  serialize_call: (ptr: number, len: number) => number;
  // eslint-disable-next-line @typescript-eslint/naming-convention
  serialize_unsigned_transaction: (ptr: number, len: number) => number;
  // eslint-disable-next-line @typescript-eslint/naming-convention
  serialize_transaction: (ptr: number, len: number) => number;
  // eslint-disable-next-line @typescript-eslint/naming-convention
  memory: Uint8Array;
};

export type AllocatedStruct = {
  bytes: Uint8Array;
  ptr: number;
};

export class SovWasm {
  // eslint-disable-next-line no-restricted-globals
  #instance: Webassembly.Instance;

  #instanceExports: WasmInstance;

  #allocs: InstanceMemorySlice[];

  // Constructor
  constructor() {
    this.#allocs = [];

    // eslint-disable-next-line no-restricted-globals
    this.#instance = new WebAssembly.Instance(
      // eslint-disable-next-line no-restricted-globals
      new WebAssembly.Module(moduleBytes),
      {
        // eslint-disable-next-line @typescript-eslint/naming-convention
        wasi_snapshot_preview1: {
          // eslint-disable-next-line @typescript-eslint/naming-convention
          fd_write: (
            _fd: number,
            _iovsPtr: number,
            _iovsLen: number,
            _nwrittenPtr: number,
          ): number => {
            return 0;
          },
          // eslint-disable-next-line @typescript-eslint/naming-convention
          environ_get: (_environ: number, _environBuf: number): number => {
            return 0;
          },
          // eslint-disable-next-line @typescript-eslint/naming-convention
          environ_sizes_get: (
            _environCount: number,
            _environSize: number,
          ): number => {
            return 0;
          },
          // eslint-disable-next-line @typescript-eslint/naming-convention
          proc_exit: (exitCode: number) => {
            throw new Error(`exit with exit code ${exitCode}`);
          },
        },
      },
    );
    this.#instanceExports = this.#instance.exports as WasmInstance;
  }

  /**
   * Allocates memory into the WASM module.
   *
   * @param len - The number of bytes to allocate.
   * @returns The pointer address of the allocated memory.
   */
  public alloc(len: number): number {
    const ptr = this.#instanceExports.alloc(len);
    this.#allocs.push([ptr, len]);
    return ptr;
  }

  /**
   * Deallocates the previously allocated WASM memory.
   */
  public dealloc(): void {
    for (const pair of this.#allocs) {
      const [ptr, len] = pair;
      this.#instanceExports.dealloc(ptr, len);
    }
    this.#allocs = [];
  }

  public bechEncodePublicKey(publicKey: string): String {
    console.trace('bechEncodePublicKey:' + JSON.stringify(publicKey));
    let bechEncodedStrBytes = this.callWasmFunction(publicKey, this.#instanceExports.bech_encode_public_key);
    return (new TextDecoder).decode(bechEncodedStrBytes);
  }

  public serializeCall(callMessage: object): Uint8Array {
    console.trace('serializeCall:' + JSON.stringify(callMessage));
    return this.callWasmFunction(callMessage, this.#instanceExports.serialize_call);
  }

  public serializeUnsignedTransaction(unsignedTransaction: object): Uint8Array {
    console.trace('serializeUnsignedTransaction:' + JSON.stringify(unsignedTransaction));
    return this.callWasmFunction(unsignedTransaction, this.#instanceExports.serialize_unsigned_transaction);
  }

  public serializeTransaction(transaction: object): Uint8Array {
    console.trace('serializeTransaction:' + JSON.stringify(transaction));
    return this.callWasmFunction(transaction, this.#instanceExports.serialize_transaction);
  }

  private callWasmFunction(args: any, wasmFunction: (ptr: number, len: number) => number): Uint8Array {
    const [argument_ptr, argument_len] = this.passArgumentsToWasmAsJson(args);
    const ptr = wasmFunction(argument_ptr, argument_len);
    return this.allocatedStruct(ptr).bytes;
  }

  /**
   *  Passes arguments to WASM environment as JSON
   *
   *  Takes the arguments object, whether a string or any JS Object, converts it into a JSON
   *  string, encodes that string to memory to be accessed from the WASM environment, and
   *  returns pointer and length so that the encoded JSON object can be retrieved from
   *  WASM side.
   * @param args
   * @private
   */
  private passArgumentsToWasmAsJson(args: any): Argument {
    const argumentsAsJsonString = JSON.stringify(args)
    const encodedArguments = (new TextEncoder()).encode(argumentsAsJsonString);
    const argumentsPtr = this.alloc(encodedArguments.length);
    const argumentsBuffer = new Uint8Array(
        this.#instanceExports.memory.buffer,
        argumentsPtr,
        encodedArguments.length,
    );
    argumentsBuffer.set(encodedArguments);
    return [argumentsPtr, encodedArguments.length];
  }

  /**
   * Reads an allocated structure, returning its bytes.
   *
   * @param ptr - The pointer address of the allocated memory.
   * @returns The WASM allocated structure.
   * @throws If the pointer is invalid.
   */
  private allocatedStruct(ptr: number): AllocatedStruct {
    if (ptr === 0) {
      throw rpcErrors.internal('Error in Serde/JSON deserialization.');
    }
    if (ptr === 1) {
      throw rpcErrors.internal('Error in serialization.');
    }
    if (ptr === 2) {
      throw rpcErrors.internal('Error when returning serialized bytes.');
    }
    const len = new DataView(this.#instanceExports.memory.buffer).getUint32(
        ptr,
        true,
    );
    const arr = this.#instanceExports.memory.buffer.slice(
        ptr + 4,
        ptr + 4 + len,
    );
    this.#allocs.push([ptr, len + 4]);
    return {
      bytes: new Uint8Array(arr),
      ptr: ptr + 4,
    };
  }
}
