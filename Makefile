SHELL := /bin/sh

BIN_NAME := llm-translator-rust
BUILD_DIR := target/release
ifeq ($(OS),Windows_NT)
	BIN_EXT := .exe
	PREFIX ?= $(USERPROFILE)/.cargo
	SETTINGS_DIR := $(USERPROFILE)/.llm-translator-rust
else
	BIN_EXT :=
	UNAME_S := $(shell uname -s)
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
	@if [ -z "$(PREFIX)" ]; then echo "PREFIX is empty; set PREFIX=/usr/local or PREFIX=$$HOME/.local"; exit 1; fi
	@mkdir -p "$(INSTALL_DIR)"
	@cp "$(BIN_PATH)" "$(INSTALL_PATH)"
	@mkdir -p "$(SETTINGS_DIR)"
	@chmod -R a+rwX "$(SETTINGS_DIR)"
	@if [ ! -f "$(SETTINGS_FILE)" ]; then cp settings.toml "$(SETTINGS_FILE)"; fi
	@echo "Installed to $(INSTALL_PATH)"

clean:
	cargo clean
