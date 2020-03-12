# Sample waSCC Actor

A sample [waSCC](https://wascc.dev/) actor that uses the AWS Lambda runtime capability provider.

## Build

#### Install [NKeys](https://github.com/encabulators/nkeys)

```console
cargo install nkeys --features "cli"
```

#### Generate Keys

```console
make keys
```

#### Add `wasm32-unknown-unknown` Compilation Target

```console
rustup target add wasm32-unknown-unknown
```

#### Install [WASCAP](https://github.com/wascc/wascap)

```console
cargo install wascap --features "cli"
```

#### Build Actor

```console
make release
```
