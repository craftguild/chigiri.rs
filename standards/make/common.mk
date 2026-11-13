# Entry protocol craftguild-entry-1; operations use craftguild-standards-engine. Include from the product repository root.
.DEFAULT_GOAL := build
CRAFTGUILD_ENTRY_DIR := $(abspath $(dir $(lastword $(MAKEFILE_LIST)))/..)
PREFIX ?= /usr/local
# The engine derives an unspecified BINDIR from the effective PREFIX.
DESTDIR ?=
# Artifact defaults belong to the engine; export only selectors supplied by callers.
export ARTIFACT VARIANT TOOLCHAIN TARGET FEATURES NO_DEFAULT_FEATURES
export PREFIX BINDIR DESTDIR
ifneq ($(origin PROFILE),undefined)
export PROFILE
endif
ifneq ($(origin CARGO_TARGET_DIR),undefined)
export CARGO_TARGET_DIR
endif
CRAFTGUILD_LAUNCH = python3 "$(CRAFTGUILD_ENTRY_DIR)/launcher.py"

.PHONY: setup prime build prepare install installer package test check acceptance release
setup:
	$(CRAFTGUILD_LAUNCH) setup
prime:
	$(CRAFTGUILD_LAUNCH) prime $(PRIME_ARGS)
build:
	$(CRAFTGUILD_LAUNCH) build $(BUILD_ARGS)
prepare:
	$(CRAFTGUILD_LAUNCH) prepare $(BUILD_ARGS)
install:
	$(CRAFTGUILD_LAUNCH) install $(INSTALL_ARGS)
installer:
	$(CRAFTGUILD_LAUNCH) installer $(INSTALLER_ARGS)
package:
	$(CRAFTGUILD_LAUNCH) package $(PACKAGE_ARGS)
test:
	$(CRAFTGUILD_LAUNCH) test
check:
	$(CRAFTGUILD_LAUNCH) check $(CHECK_ARGS)
acceptance:
	$(CRAFTGUILD_LAUNCH) acceptance $(ACCEPTANCE_ARGS)
release:
	$(CRAFTGUILD_LAUNCH) release $(or $(RELEASE_ARGS),--apply)
