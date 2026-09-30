.PHONY: lint test test-bash test-pwsh test-site ci-local

SHELLCHECK ?= shellcheck
BATS ?= bats

SHELL_SCRIPTS := apps/path/test-network-path.sh $(wildcard scripts/*.sh) $(wildcard src/bash/path/*.sh) $(wildcard src/bash/path/lib/*.sh) tests/path/bash/test_helper.bash $(wildcard tests/path/bash/fakes/mtr-*)

lint:
	$(SHELLCHECK) -x $(SHELL_SCRIPTS)

test-bash:
	$(BATS) tests/path/contracts.bats tests/path/bash

test-pwsh:
	pwsh -NoProfile -NonInteractive -File scripts/ci.ps1 -NoInstall

test-site:
	node --test tests/site/*.test.cjs

test: test-bash test-site test-pwsh

ci-local:
	./scripts/ci-local.sh
