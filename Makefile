SHELL := /bin/sh

BIN_NAME := llm-translator-rust
BUILD_DIR := target/release

ifeq ($(OS),Windows_NT)
	BIN_EXT := .exe
	PREFIX ?= $(USERPROFILE)/.cargo
	SETTINGS_DIR := $(USERPROFILE)/.llm-translator-rust
else
	BIN_EXT :=
	PREFIX ?= /usr/local
	SETTINGS_DIR := $(HOME)/.llm-translator-rust
endif

BIN_PATH := $(BUILD_DIR)/$(BIN_NAME)$(BIN_EXT)
INSTALL_DIR := $(PREFIX)/bin
INSTALL_PATH := $(INSTALL_DIR)/$(BIN_NAME)$(BIN_EXT)
SETTINGS_FILE := $(SETTINGS_DIR)/settings.toml

.PHONY: install clean build

build:
	cargo build --release

install: build
	@mkdir -p "$(INSTALL_DIR)"
	@cp "$(BIN_PATH)" "$(INSTALL_PATH)"
	@mkdir -p "$(SETTINGS_DIR)"
	@if [ ! -f "$(SETTINGS_FILE)" ]; then cp settings.toml "$(SETTINGS_FILE)"; fi
	@echo "Installed to $(INSTALL_PATH)"

clean:
	cargo clean
