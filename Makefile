ARTIFACT ?= chigiri-cli
RELEASE_GROUP ?= chigiri
RELEASE_ARGS ?= --apply
override RELEASE_ARGS := --group $(RELEASE_GROUP) $(RELEASE_ARGS)
ACCEPTANCE_ARGS ?= --gate cli-standalone-acceptance
include standards/make/common.mk
CRAFTGUILD_LAUNCH = sh tools/standards.sh

.PHONY: debug macos-arm64 macos-x86_64 linux-x86_64 windows-x86_64 cross check-build-tools verify-dist
debug:
	$(MAKE) build PROFILE=debug
acceptance: build
macos-arm64:
	./scripts/build-distribution.sh aarch64-apple-darwin
macos-x86_64:
	./scripts/build-distribution.sh x86_64-apple-darwin
linux-x86_64:
	./scripts/build-distribution.sh x86_64-unknown-linux-gnu
windows-x86_64:
	./scripts/build-distribution.sh x86_64-pc-windows-msvc
cross:
	./scripts/check-build-tools.sh all
	./scripts/build-distribution.sh aarch64-apple-darwin
	./scripts/build-distribution.sh x86_64-apple-darwin
	./scripts/build-distribution.sh x86_64-unknown-linux-gnu
	./scripts/build-distribution.sh x86_64-pc-windows-msvc
	./scripts/verify-dist.py
check-build-tools:
	./scripts/check-build-tools.sh all
verify-dist:
	./scripts/verify-dist.py
