.PHONY: dev update push clean

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
