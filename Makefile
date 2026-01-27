SHELL := /bin/sh

BIN_NAME := llm-translator-rust
BUILD_DIR := target/release

ifeq ($(OS),Windows_NT)
	BIN_EXT := .exe
	PREFIX ?= $(USERPROFILE)/.cargo
else
	BIN_EXT :=
	PREFIX ?= /usr/local
endif

BIN_PATH := $(BUILD_DIR)/$(BIN_NAME)$(BIN_EXT)
INSTALL_DIR := $(PREFIX)/bin
INSTALL_PATH := $(INSTALL_DIR)/$(BIN_NAME)$(BIN_EXT)

.PHONY: install clean build

build:
	cargo build --release

install: build
	@mkdir -p "$(INSTALL_DIR)"
	@cp "$(BIN_PATH)" "$(INSTALL_PATH)"
	@echo "Installed to $(INSTALL_PATH)"

clean:
	cargo clean
