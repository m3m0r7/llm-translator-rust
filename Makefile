SHELL := /bin/sh

BIN_NAME := llm-translator-rust
BUILD_DIR := target/release
BUILD_TOOL_NAME := llm-translator-build
BUILD_TOOL_MANIFEST := build/Cargo.toml
BUILD_TOOL_BIN := build/target/release/$(BUILD_TOOL_NAME)
BUILD_ENV_PATH ?= build/build_env.toml
BUILD_TOOL_BUILDER ?= cargo
BUILD_TARGET ?=
BUILD_BIN_DIR ?= $(BUILD_DIR)
DATA_DIRECTORY ?=
BIN_DIRECTORY ?=
RUNTIME_DIRECTORY ?=
CONFIG_DIRECTORY ?=
SETTINGS_FILE ?=
BASE_DIRECTORY ?=
INSTALL_DIRECTORY ?=
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
ifneq ($(strip $(DATA_DIRECTORY)),)
	BUILD_ENV_ARGS += --data-directory "$(DATA_DIRECTORY)"
else ifneq ($(strip $(BASE_DIRECTORY)),)
	BUILD_ENV_ARGS += --data-directory "$(BASE_DIRECTORY)"
endif
ifneq ($(strip $(BIN_DIRECTORY)),)
	BUILD_ENV_ARGS += --bin-directory "$(BIN_DIRECTORY)"
endif
ifneq ($(strip $(RUNTIME_DIRECTORY)),)
	BUILD_ENV_ARGS += --runtime-directory "$(RUNTIME_DIRECTORY)"
else ifneq ($(strip $(INSTALL_DIRECTORY)),)
	BUILD_ENV_ARGS += --runtime-directory "$(INSTALL_DIRECTORY)"
endif
ifneq ($(strip $(CONFIG_DIRECTORY)),)
	BUILD_ENV_ARGS += --config-directory "$(CONFIG_DIRECTORY)"
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
	@if [ -f "$(BUILD_ENV_PATH)" ]; then \
		if [ -f "$(BUILD_TOOL_BIN)" ]; then \
			"$(BUILD_TOOL_BIN)" clean --env-path "$(BUILD_ENV_PATH)" --project-dir "$(CURDIR)"; \
		else \
			cargo run --manifest-path "$(BUILD_TOOL_MANIFEST)" -- clean --env-path "$(BUILD_ENV_PATH)" --project-dir "$(CURDIR)"; \
		fi; \
	fi
	cargo clean
	@cargo clean --manifest-path "$(BUILD_TOOL_MANIFEST)" || true
	@rm -f "$(BUILD_ENV_PATH)"
