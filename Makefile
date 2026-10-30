.PHONY: nested-session

nested-session:
	cargo build --target-dir target
	bash tools/nested-session.sh
