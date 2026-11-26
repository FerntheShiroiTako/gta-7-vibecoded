# THIS IS NOT A PROPER PROJECT, NOR ONE THAT I AM PROUD OF
## No lines were written by me, it was just a simple test of claude ais abilities 

## First run/install steps:

1. Install Rust (stable, MSVC) from https://rustup.rs, then:
rustup default stable-x86_64-pc-windows-msvc

2. DLSS SDK, pinned version, into ./DLSS
git clone https://github.com/NVIDIA/DLSS.git DLSS
git -C DLSS checkout v310.9.1

## To play run
cargo run --release