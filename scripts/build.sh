#!/bin/bash

cargo clean

# MacOS
cargo build -r

# Windows
cargo build --target x86_64-pc-windows-gnu -r

# Linux
cargo build --target x86_64-unknown-linux-gnu -r

# Just copying binaries to root folder
cp target/release/rust_dart_gen rust_dart_gen_macos
cp target/x86_64-pc-windows-gnu/release/rust_dart_gen.exe rust_dart_gen_windows
cp target/x86_64-unknown-linux-gnu/release/rust_dart_gen rust_dart_gen_linux
