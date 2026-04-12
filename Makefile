.PHONY: lint validate test test-bash test-pwsh ci-local

SHELLCHECK ?= shellcheck
BATS ?= bats

SHELL_SCRIPTS := apps/path/mtr-test-suite.sh scripts/ci-local.sh scripts/install-test-deps.sh scripts/run-workflow.sh $(wildcard src/bash/path/lib/*.sh)

lint:
	$(SHELLCHECK) -x $(SHELL_SCRIPTS)

validate:
	$(SHELLCHECK) -x $(SHELL_SCRIPTS)

test-bash:
	$(BATS) tests/path

test-pwsh:
	pwsh -NoProfile -NonInteractive -File scripts/ci.ps1

test: test-bash test-pwsh

ci-local:
	./scripts/ci-local.sh
