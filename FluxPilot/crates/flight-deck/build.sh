#!/bin/bash
export RUSTFLAGS="-C link-arg=-zstack-size=2097152"
wasm-pack build --target web
