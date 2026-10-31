CARGO ?= $(shell command -v cargo 2>/dev/null || echo $(HOME)/.cargo/bin/cargo)
GO ?= go
CARGO_FLAGS ?= --release
PROFILE_DIR := release

ifeq ("$(DEBUG)", "true")
	CARGO_FLAGS :=
	PROFILE_DIR := debug
endif

RUST_BINS := \
	syzaster \
	check-asterinas-kcov \
	check-kcov-inst \
	kcov-rustc \
	eval-fuzz

.PHONY: all rust-tools syzkaller patch-syzkaller unpatch-syzkaller test clean prepare

all: rust-tools

prepare:
	@mkdir -p bin

rust-tools: prepare
	$(CARGO) build $(CARGO_FLAGS)
	@for b in $(RUST_BINS); do \
		cp -f target/$(PROFILE_DIR)/$$b bin/$$b; \
	done

patch-syzkaller:
	@cd syzkaller && \
	for p in ../patch/syzkaller/*.patch; do \
		if ! git apply --reverse --check "$$p" >/dev/null 2>&1; then \
			echo "Applying $$(basename $$p)..."; \
			git apply "$$p"; \
		else \
			echo "Already applied: $$(basename $$p)"; \
		fi; \
	done

unpatch-syzkaller:
	cd syzkaller && git clean -fd && git reset --hard HEAD && rm -f pkg/mgrconfig/testdata/qemu-asterinas.cfg

syzkaller: patch-syzkaller
	$(MAKE) -C syzkaller TARGETOS=linux TARGETARCH=amd64

test:
	$(CARGO) test

clean:
	rm -rf bin target
