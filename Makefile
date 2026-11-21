BINARY_NAME := mac-tui-fm
INSTALL_DIR := /usr/local/bin
CONFIG_DIR := $(HOME)/.config/mac-tui-fm

.PHONY: build install uninstall clean check test config

build:
	cargo build --release

check:
	cargo check

test:
	cargo test

install: build config
	install -m 755 target/release/$(BINARY_NAME) $(INSTALL_DIR)/$(BINARY_NAME)
	@echo "Installed $(BINARY_NAME) to $(INSTALL_DIR)/$(BINARY_NAME)"

config:
	mkdir -p $(CONFIG_DIR)
	@if [ ! -f $(CONFIG_DIR)/keybindings.json ]; then \
		cp config/keybindings.json $(CONFIG_DIR)/keybindings.json; \
		echo "Installed default keybindings.json to $(CONFIG_DIR)"; \
	fi
	@if [ ! -f $(CONFIG_DIR)/projects.json ]; then \
		cp config/projects.json $(CONFIG_DIR)/projects.json; \
		echo "Installed default projects.json to $(CONFIG_DIR)"; \
	fi

uninstall:
	rm -f $(INSTALL_DIR)/$(BINARY_NAME)

clean:
	cargo clean
