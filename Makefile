.PHONY: dev update push clean push-dev push-test push-beta _push-channel linux windows macos

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

# Channel snapshot: no VERSION bump. Tag vX.Y.Z-{channel}[-platform...].
_push-channel:
	@if [ -z "$(CHANNEL)" ]; then echo ">> CHANNEL is required"; exit 1; fi
	@VERSION=$$(cat VERSION); \
	TAG="v$$VERSION-$(CHANNEL)"; \
	for p in $(PLATFORMS); do TAG="$$TAG-$$p"; done; \
	if git rev-parse "$$TAG" >/dev/null 2>&1; then \
		echo ">> Tag $$TAG already exists"; \
		exit 1; \
	fi; \
	if [ -n "$$(git status --porcelain)" ]; then \
		MSG="$(MSG)"; \
		COMMIT_MSG=$${MSG:-"$(CHANNEL) snapshot $$VERSION"}; \
		git add -A; \
		git commit -m "$$COMMIT_MSG"; \
	fi; \
	git tag "$$TAG"; \
	git push && git push origin "$$TAG"; \
	echo ">> Released $$TAG"
