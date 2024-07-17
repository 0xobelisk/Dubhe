# `sov-nonces` module

The `sov-nonces` module is responsible for managing nonces on the rollup.

The module does not expose any `CallMessage` therefore, its state can't be directly modified by the users of the rollup. Instead the nonces are modified via the rollup's capabilities.