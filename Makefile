.PHONY: dev dev-profile update push clean \
	alpha beta rc release linux windows macos android ios \
	android-dev android-test android-release android-devices android-screenshot

# Flags for `make push alpha|beta|rc|release [linux] [windows] [macos] [android] [ios]`
alpha beta rc release linux windows macos android ios:
	@:

PUSH_CHANNELS := $(filter alpha beta rc release,$(MAKECMDGOALS))
PUSH_CHANNEL := $(firstword $(PUSH_CHANNELS))
PUSH_PLATFORMS := $(strip $(foreach p,linux windows macos android ios,$(filter $(p),$(MAKECMDGOALS))))

clean:
	@echo ">> Stopping running processes..."
	@bash -c 'SELF=$$$$; pgrep -f "veydanspace" 2>/dev/null | while read pid; do [ "$$pid" != "$$SELF" ] && kill "$$pid" 2>/dev/null; done; exit 0'
	@echo ">> Removing build artifacts..."
	@rm -rf src-tauri/target
	@rm -rf build
	@echo ">> Done"

dev:
	@bash dev.sh

# One more dev instance with its own data directory, next to a running
# `make dev`. usage: make dev-profile WORKDIR=<dir>
dev-profile:
	@bash scripts/run-profile.sh "$(WORKDIR)"

# TypeScript types of what the messenger runtime hands to the UI, from the Rust types.
msg-types:
	@bash -c 'source build-env.sh >/dev/null && UPDATE_TS_BINDINGS=1 cargo test --manifest-path src-tauri/crates/messenger/Cargo.toml -p messenger-runtime bindings -- --quiet'

# Messenger crates: Tauri-free workspace, testable without the app.
msg-test:
	@bash -c 'source build-env.sh >/dev/null && cargo test --workspace --manifest-path src-tauri/crates/messenger/Cargo.toml'

msg-check:
	@bash scripts/messenger-boundaries.sh

android-dev:
	@bash scripts/android/dev.sh

android-test:
	@bash scripts/android/apk.sh test

android-release:
	@bash scripts/android/apk.sh release

android-devices:
	@bash -c 'source scripts/android/android-env.sh && adb devices'

# usage: make android-screenshot  → screenshots/N.png (next free number)
android-screenshot:
	@bash -c 'source scripts/android/android-env.sh && \
		mkdir -p screenshots && \
		n=1; while [ -e "screenshots/$$n.png" ]; do n=$$((n+1)); done; \
		OUT="screenshots/$$n.png" && \
		adb exec-out screencap -p > "$$OUT" && echo ">> $$OUT"'

update:
	@bash update.sh

# usage: make push alpha|beta|rc|release [linux] [windows] [macos] [android] [ios]
push:
	@if [ "$(words $(PUSH_CHANNELS))" -gt 1 ]; then \
		echo ">> usage: make push alpha|beta|rc|release [linux] [windows] [macos] [android] [ios]"; \
		exit 1; \
	fi
	@bash scripts/push.sh "$(PUSH_CHANNEL)" "$(PUSH_PLATFORMS)" "$(VERSION)" "$(MSG)"

# VPush (push server) lives in VPush/ with its own Makefile:
# make vpush-test, make vpush-deploy, make vpush-logs ...
vpush-%:
	@$(MAKE) --no-print-directory -C VPush $* ARGS="$(ARGS)"

# VPush binary for the server kit: deploy/bin/vpush (see deploy/README.md).
.PHONY: build-deploy
build-deploy:
	@$(MAKE) --no-print-directory -C VPush build-deploy
