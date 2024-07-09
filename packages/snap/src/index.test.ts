import { expect } from '@jest/globals';
import { installSnap } from '@metamask/snaps-jest';

describe('onRpcRequest', () => {
  it('throws an error if the requested method does not exist', async () => {
    const { request, close } = await installSnap();

    const response = await request({
      method: 'foo',
    });

    expect(response).toRespondWithError({
      code: -32603,
      message: 'Method not found.',
      stack: expect.any(String),
    });

    await close();
  });

  describe('getAddress', () => {
    it('returns an address', async () => {
      const { request, close } = await installSnap();

      const response = request({
        method: 'getAddress',
        params: {
          keyId: 0,
        },
      });

      expect(await response).toRespondWith(
        'sov1dpvv2cvyv3cfsflx4hz67gyqcm3xlrj5v2ldyagep2uf8fwt7drsv936j0',
      );

      await close();
    });
  });

  describe('signTransaction', () => {
    it('returns a signed transaction', async () => {
      const { request, close } = await installSnap();

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

      const ui = await response.getInterface();
      expect(ui.type).toBe('confirmation');

      await ui.ok();

      expect(await response).toRespondWith(
        '0x5245f438f625e2b272765a66f99484668cc162466a3ab0751603353c836de62d42165f1be154f9fb3b87f32ce39c9743c41a90d2129daee7db0aa55155e74e05e9cf61f2b4c9402e3b4742b44ec2b8cbc6184a3a6d7c64c7e2e371f6cae160ff8800000001000b000000000000000e000000736f762d746573742d746f6b656e40420f0000000000a3201954f70ad62230dc3d840a5bf767702c04869e85ab3eee0b962857ba759802000000fea6ac5b8751120fb62fff67b54d2eac66aef307c7dde1d394dea1e09e43dd44a3201954f70ad62230dc3d840a5bf767702c04869e85ab3eee0b962857ba759800000000000000000000000000000000b80b000000000000000000000000000000',
      );

      await close();
    });
  });
});
