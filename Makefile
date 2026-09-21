.PHONY: dev update push clean \
	alpha beta rc release linux windows macos android ios \
	android-dev android-test android-release android-devices

# Flags for `make push alpha|beta|rc|release [linux] [windows] [macos] [android] [ios]`
alpha beta rc release linux windows macos android ios:
	@:

PUSH_CHANNELS := $(filter alpha beta rc release,$(MAKECMDGOALS))
PUSH_CHANNEL := $(firstword $(PUSH_CHANNELS))
PUSH_PLATFORMS := $(strip $(foreach p,linux windows macos android ios,$(filter $(p),$(MAKECMDGOALS))))

clean:
	@echo ">> Stopping running processes..."
	@bash -c 'SELF=$$$$; pgrep -f "veydanbrowser" 2>/dev/null | while read pid; do [ "$$pid" != "$$SELF" ] && kill "$$pid" 2>/dev/null; done; exit 0'
	@echo ">> Removing build artifacts..."
	@rm -rf src-tauri/target
	@rm -rf build
	@echo ">> Done"

dev:
	@bash dev.sh

android-dev:
	@bash scripts/android/dev.sh

android-test:
	@bash scripts/android/apk.sh test

android-release:
	@bash scripts/android/apk.sh release

android-devices:
	@bash -c 'source scripts/android/android-env.sh && adb devices'

update:
	@bash update.sh

# usage: make push alpha|beta|rc|release [linux] [windows] [macos] [android] [ios]
push:
	@if [ "$(words $(PUSH_CHANNELS))" -gt 1 ]; then \
		echo ">> usage: make push alpha|beta|rc|release [linux] [windows] [macos] [android] [ios]"; \
		exit 1; \
	fi
	@bash scripts/push.sh "$(PUSH_CHANNEL)" "$(PUSH_PLATFORMS)" "$(VERSION)" "$(MSG)"
