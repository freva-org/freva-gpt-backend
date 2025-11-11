.PHONY: release

release:
	@VERSION=$$(curl -s https://raw.githubusercontent.com/freva-org/freva-gpt-backend/refs/heads/main/Cargo.toml | grep '^version' | head -1 | cut -d'"' -f2); \
	git tag "v$$VERSION"; \
	git push origin "v$$VERSION"; \
	echo "bumped v$$VERSION"