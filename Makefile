SHELL := /bin/sh

BIN_NAME := llm-translator-rust
BUILD_DIR := target/release
BUILD_TOOL_NAME := llm-translator-build
BUILD_TOOL_MANIFEST := build/Cargo.toml
BUILD_TOOL_BIN := build/target/release/$(BUILD_TOOL_NAME)
BUILD_ENV_PATH ?= build_env.toml
BUILD_TOOL_BUILDER ?= cargo
BUILD_TARGET ?=
BUILD_BIN_DIR ?= $(BUILD_DIR)
BASE_DIRECTORY ?=
BIN_DIRECTORY ?=
INSTALL_DIRECTORY ?=
SETTINGS_FILE ?=
ifeq ($(OS),Windows_NT)
	BIN_EXT := .exe
	BUILD_TOOL_BIN := $(BUILD_TOOL_BIN).exe
else
	BIN_EXT :=
	UNAME_S := $(shell uname -s)
endif

BUILD_TARGET_ARG :=
ifneq ($(strip $(BUILD_TARGET)),)
	BUILD_TARGET_ARG := --target "$(BUILD_TARGET)"
	BUILD_BIN_DIR := target/$(BUILD_TARGET)/release
endif

BUILD_ENV_ARGS :=
ifneq ($(strip $(BASE_DIRECTORY)),)
	BUILD_ENV_ARGS += --base-directory "$(BASE_DIRECTORY)"
endif
ifneq ($(strip $(BIN_DIRECTORY)),)
	BUILD_ENV_ARGS += --bin-directory "$(BIN_DIRECTORY)"
endif
ifneq ($(strip $(INSTALL_DIRECTORY)),)
	BUILD_ENV_ARGS += --install-directory "$(INSTALL_DIRECTORY)"
endif
ifneq ($(strip $(SETTINGS_FILE)),)
	BUILD_ENV_ARGS += --settings-file "$(SETTINGS_FILE)"
endif

.PHONY: install clean build build-tool

build: build-tool
	@$(BUILD_TOOL_BIN) build --env-path "$(BUILD_ENV_PATH)" --bin-directory "$(BUILD_BIN_DIR)" --project-dir "$(CURDIR)" --builder "$(BUILD_TOOL_BUILDER)" $(BUILD_TARGET_ARG) $(BUILD_ENV_ARGS)

build-tool: $(BUILD_TOOL_BIN)

$(BUILD_TOOL_BIN): build/Cargo.toml build/Cargo.lock build/main.rs
	@cargo build --release --manifest-path "$(BUILD_TOOL_MANIFEST)"

install:
	@if [ ! -f "$(BUILD_ENV_PATH)" ]; then echo "$(BUILD_ENV_PATH) not found. Run 'make' first."; exit 1; fi
	@$(BUILD_TOOL_BIN) install --env-path "$(BUILD_ENV_PATH)" --bin-name "$(BIN_NAME)" --bin-ext "$(BIN_EXT)" --project-dir "$(CURDIR)"

clean:
	cargo clean
	@cargo clean --manifest-path "$(BUILD_TOOL_MANIFEST)" || true
	@rm -f "$(BUILD_ENV_PATH)"
