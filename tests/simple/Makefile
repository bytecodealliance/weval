NAME := simple
COMPONENT_NAME := simple-component
WIT_BINDINGS_DIR := $(COMPONENT_NAME)-bindings

CC := /opt/wasi-sdk/bin/clang
CXX := /opt/wasi-sdk/bin/clang++
CFLAGS := -I../../include -O2 -g
CXXFLAGS := $(CFLAGS) -std=c++23
WEVAL := ../../target/release/weval
WASM_TOOLS ?= wasm-tools
WASMTIME ?= wasmtime
WIT_BINDGEN ?= wit-bindgen
COMPONENT_CXXFLAGS := --target=wasm32-wasip2 -DWEVAL_COMPONENT_TEST -I../../include -I$(WIT_BINDINGS_DIR) -O2 -g -std=c++23 -mexec-model=reactor

.PHONY: all
all: $(NAME).wasm $(NAME)-wevaled.wasm $(COMPONENT_NAME)-wevaled.wasm

$(NAME).wasm: $(NAME).o
	$(CXX) $(CXXFLAGS) -o $@ $^

$(NAME)-wevaled.wasm: $(NAME).wasm
	$(WEVAL) weval -w -i $^ -o $@

$(NAME).o: $(NAME).cpp ../../include/weval.h
	$(CXX) $(CXXFLAGS) -c -o $@ $<

weval-stubs.wasm: ../../lib/weval-stubs.wat
	$(WASM_TOOLS) parse $< -o $@

$(COMPONENT_NAME).wasm: $(NAME).cpp ../../include/weval.h component.wit weval-stubs.wasm
	rm -rf $(WIT_BINDINGS_DIR)
	mkdir -p $(WIT_BINDINGS_DIR)
	$(WIT_BINDGEN) cpp --out-dir $(WIT_BINDINGS_DIR) component.wit
	$(CXX) $(COMPONENT_CXXFLAGS) -o $@ \
		-Wl,--adapt,weval=weval-stubs.wasm \
		$(NAME).cpp \
		$(WIT_BINDINGS_DIR)/component.cpp \
		$(WIT_BINDINGS_DIR)/component_component_type.o

$(COMPONENT_NAME)-wevaled.wasm: $(COMPONENT_NAME).wasm
	$(WEVAL) weval -w --init-func initialize --keep-init-func --allow-wasip2 -i $< -o $@
	$(WASM_TOOLS) validate $@

.PHONY: clean
clean:
	rm -rf *.wasm *.o $(WIT_BINDINGS_DIR)

.PHONY: run-base
run-base: $(NAME).wasm
	$(WASMTIME) run --preload weval=../../lib/weval-stubs.wat $(NAME).wasm

.PHONY: run-wevaled
run-wevaled: $(NAME)-wevaled.wasm
	$(WASMTIME) run $(NAME)-wevaled.wasm

.PHONY: run-component
run-component: $(COMPONENT_NAME)-wevaled.wasm
	$(WASMTIME) run --invoke 'run()' $(COMPONENT_NAME)-wevaled.wasm
