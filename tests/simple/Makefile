NAME := simple

CC := /opt/wasi-sdk/bin/clang
CXX := /opt/wasi-sdk/bin/clang++
CFLAGS := -I../../include -O2 -g
CXXFLAGS := $(CFLAGS) -std=c++17
WEVAL := ../../target/release/weval

.PHONY: all
all: $(NAME).wasm $(NAME)-wevaled.wasm

$(NAME).wasm: $(NAME).o
	$(CXX) $(CXXFLAGS) -o $@ $^

$(NAME)-wevaled.wasm: $(NAME).wasm
	$(WEVAL) weval -w -i $^ -o $@

$(NAME).o: $(NAME).cpp ../../include/weval.h
	$(CXX) $(CXXFLAGS) -c -o $@ $<

.PHONY: clean
clean:
	rm -f $(NAME).wasm $(NAME)-wevaled.wasm *.o

.PHONY: run-base
run-base: $(NAME).wasm
	wasmtime run --preload weval=../../lib/weval-stubs.wat $(NAME).wasm

.PHONY: run-wevaled
run-wevaled: $(NAME)-wevaled.wasm
	wasmtime run $(NAME)-wevaled.wasm

