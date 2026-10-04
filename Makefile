.PHONY: lint test test-pwsh ci-local

SHELLCHECK ?= shellcheck

SHELL_SCRIPTS := apps/path/test-network-path.sh $(wildcard scripts/*.sh) $(wildcard src/bash/path/*.sh) $(wildcard src/bash/path/lib/*.sh)

lint:
	$(SHELLCHECK) -x $(SHELL_SCRIPTS)

test-pwsh:
	pwsh -NoProfile -NonInteractive -File scripts/ci.ps1 -NoInstall

test: test-pwsh

ci-local:
	./scripts/ci-local.sh
