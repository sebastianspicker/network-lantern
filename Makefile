.PHONY: lint test test-bash test-pwsh ci-local

SHELLCHECK ?= shellcheck
BATS ?= bats
VENDORED_BATS := tests/bats/bin/bats

SHELL_SCRIPTS := apps/path/mtr-test-suite.sh scripts/ci-local.sh scripts/install-test-deps.sh scripts/run-workflow.sh $(wildcard src/bash/path/lib/*.sh)

lint:
	$(SHELLCHECK) -x $(SHELL_SCRIPTS)

test-bash:
	@if command -v "$(BATS)" >/dev/null 2>&1; then \
		"$(BATS)" tests/path; \
	elif [ -x "$(VENDORED_BATS)" ]; then \
		"$(VENDORED_BATS)" tests/path; \
	else \
		echo "bats not found. Install bats or run scripts/install-test-deps.sh." >&2; \
		exit 1; \
	fi

test-pwsh:
	pwsh -NoProfile -NonInteractive -File scripts/ci.ps1

test: test-bash test-pwsh

ci-local:
	./scripts/ci-local.sh
