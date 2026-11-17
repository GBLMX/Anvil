APP := anvil
SRC := $(wildcard src/*.rs)

.PHONY: build test clean

build:
	cargo build --release

test:
	cargo test

clean:
	rm -rf target
