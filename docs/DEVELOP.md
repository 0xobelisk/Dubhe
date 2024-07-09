# Developing MetaMask Snap

During development of your snap it can be useful to make the snap available to end users for testing (Note: they should be using [MetaMask Flask](https://support.metamask.io/getting-started/what-is-metamask-flask-and-how-is-it-different-from-normal-metamask/) as they will be testing a development snap build).

The following will provide instructions on how to achieve this assuming you're using this repository as a template and have made progress in developing your snap (i.e you're familiar with generating WASM for your rollup), although they should still serve as guidance if your setup differs.

You will need an [npm](https://www.npmjs.com/) account.

## Preparing to publish

Update the `name` field in `packages/snap/package.json`, this will be the name of the repository on npm.

OPTIONAL: If your snap is still under going active development it could be useful to tag npm releases as `alpha` versions by updating the `version` field in `packages/snap/package.json` to something like `1.0.0-alpha.0` and then including the `alpha` tag when publishing.

## Building your snap

After generating the WASM for your rollup configuration (via running `yarn update-wasm` in the root directory) you can bundle your snap for distribution.

Go back the root directory and then execute build:

**NOTE**: If you've changed the `name` field in `packages/snap/package.json`, you should change the workspace name `sov-snap` to match your new `name` field in the `yarn build` script, which is defined in the root directory's `package.json`.
```sh
cd ../..
yarn build
```

If successful your snap will be bundled for publishing to npm.

## Publishing to NPM

Now we will publish the bundle to npm. `cd` back into the snap directory:

```sh
cd packages/snap
```

If you have not already done so, login to `npm` by running:

```sh
npm login
```

Now publish your snap, optionally supplying a tag:

```sh
npm publish --tag alpha
```

If all went well your snap npm package should be available at `https://www.npmjs.com/package/$YOUR_PACKAGE_NAME`

## Update frontend code that connects to the snap

To use your newly published snap in your dapp frontend you should specify the `wallet_requestSnaps` requests. [See more here](https://docs.metamask.io/snaps/how-to/request-permissions/#request-permissions-from-a-dapp). In short, you can utilize your snap by replacing the two `npm:hello-snap` instances in the linked example with `npm:$YOUR_NPM_SNAP_PACKAGE_NAME`.

If you're using the `gatsby` template provided in the `site` package in this repository it can be done by specifying a env var in `packages/site/.env.development` for dev builds or `packages/site/.env.production` for prod builds.

```
SNAP_ORIGIN="npm:$YOUR_NPM_SNAP_PACKAGE_NAME"
```

Where `$NPM_PACKAGE_NAME` is the name of your snap package published to npm.

## Finish

If all went well when users connect their wallet on your dApp frontend they will be able to connect to the published snap.
