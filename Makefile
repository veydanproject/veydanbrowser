.PHONY: dev update push clean push-dev push-test push-beta _push-channel linux windows macos android-dev android-apk android-install android-devices

# So `make push-dev linux` treats linux/windows/macos as flags, not real targets.
linux windows macos:
	@:

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
	@bash mobile/dev.sh

android-apk:
	@bash -c 'source mobile/android-env.sh && cd mobile && pnpm tauri android build'

android-install:
	@bash -c 'source mobile/android-env.sh && \
	  apk=$$(find mobile/src-tauri/gen/android/app/build/outputs/apk -name "*.apk" -printf "%T@\t%p\n" 2>/dev/null | sort -nr | cut -f2- | head -n1); \
	  if [ -z "$$apk" ]; then echo ">> No APK. Run: make android-apk"; exit 1; fi; \
	  echo ">> Installing $$apk"; \
	  adb install -r "$$apk"'

android-devices:
	@bash -c 'source mobile/android-env.sh && adb devices'

update:
	@bash update.sh

push:
	@CURRENT=$$(cat VERSION); \
	if [ -n "$(VERSION)" ]; then \
		NEXT="$(VERSION)"; \
	else \
		MAJOR=$$(echo $$CURRENT | cut -d. -f1); \
		MINOR=$$(echo $$CURRENT | cut -d. -f2); \
		PATCH=$$(echo $$CURRENT | cut -d. -f3); \
		NEXT="$$MAJOR.$$MINOR.$$((PATCH + 1))"; \
	fi; \
	echo ">> Bumping $$CURRENT → $$NEXT"; \
	echo "$$NEXT" > VERSION; \
	sed -i 's/"version": "[^"]*"/"version": "'"$$NEXT"'"/' package.json; \
	sed -i 's/"version": "[^"]*"/"version": "'"$$NEXT"'"/' src-tauri/tauri.conf.json; \
	sed -i '0,/^version = "[^"]*"/{s/^version = "[^"]*"/version = "'"$$NEXT"'"/}' src-tauri/Cargo.toml; \
	MSG="$(MSG)"; \
	COMMIT_MSG=$${MSG:-"release $$NEXT"}; \
	git add -A; \
	git commit -m "$$COMMIT_MSG"; \
	git tag v$$NEXT; \
	git push && git push origin v$$NEXT; \
	echo ">> Released v$$NEXT"

push-dev push-test push-beta:
	@$(MAKE) _push-channel CHANNEL=$(patsubst push-%,%,$@) PLATFORMS="$(filter linux windows macos,$(MAKECMDGOALS))"

# Channel snapshot: no VERSION bump. Tag vX.Y.Z-{channel}[-platform...]-N
_push-channel:
	@if [ -z "$(CHANNEL)" ]; then echo ">> CHANNEL is required"; exit 1; fi
	@VERSION=$$(cat VERSION); \
	PREFIX="v$$VERSION-$(CHANNEL)"; \
	for p in $(PLATFORMS); do PREFIX="$$PREFIX-$$p"; done; \
	MAX=0; \
	for t in $$(git tag -l "$$PREFIX-*"); do \
		n=$${t#$$PREFIX-}; \
		case "$$n" in \
			''|*[!0-9]*) ;; \
			*) if [ "$$n" -gt "$$MAX" ]; then MAX=$$n; fi ;; \
		esac; \
	done; \
	REMOTE=$$(git ls-remote --tags origin "refs/tags/$${PREFIX}-*" 2>/dev/null | cut -f2 | sed -e 's#^refs/tags/##' -e 's#\^{}$$##'); \
	for t in $$REMOTE; do \
		n=$${t#$$PREFIX-}; \
		case "$$n" in \
			''|*[!0-9]*) ;; \
			*) if [ "$$n" -gt "$$MAX" ]; then MAX=$$n; fi ;; \
		esac; \
	done; \
	TAG="$$PREFIX-$$((MAX + 1))"; \
	if [ -n "$$(git status --porcelain)" ]; then \
		MSG="$(MSG)"; \
		COMMIT_MSG=$${MSG:-"$(CHANNEL) snapshot $$VERSION"}; \
		git add -A; \
		git commit -m "$$COMMIT_MSG"; \
	fi; \
	git tag "$$TAG"; \
	git push && git push origin "$$TAG"; \
	echo ">> Released $$TAG"
