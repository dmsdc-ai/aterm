.PHONY: all clean run rust swift app

# Paths
ROOT := $(shell pwd)
CORE_DIR := $(ROOT)/aterm-core
MACOS_DIR := $(ROOT)/macos
BUILD_DIR := $(ROOT)/build
APP_DIR := $(BUILD_DIR)/aterm.app/Contents
TARGET_DIR := $(ROOT)/target/release

# Rust
DYLIB := $(TARGET_DIR)/libaterm_core.dylib

# Swift sources
SWIFT_SOURCES := $(wildcard $(MACOS_DIR)/Sources/*.swift)
BRIDGE_HEADER := $(MACOS_DIR)/aterm-bridge.h

all: app

# Step 1: Build Rust cdylib
rust:
	cd $(CORE_DIR) && cargo build --release
	@echo "[build] Rust cdylib built: $(DYLIB)"

# Step 2: Compile Swift
swift: rust
	@mkdir -p $(BUILD_DIR)
	swiftc \
		-O \
		-target arm64-apple-macosx14.0 \
		-import-objc-header $(BRIDGE_HEADER) \
		-L $(TARGET_DIR) \
		-laterm_core \
		-framework AppKit \
		-framework Foundation \
		-framework Metal \
		-framework QuartzCore \
		-framework CoreVideo \
		$(SWIFT_SOURCES) \
		-o $(BUILD_DIR)/aterm
	@echo "[build] Swift binary built: $(BUILD_DIR)/aterm"

# Step 3: Create .app bundle
app: swift
	@mkdir -p $(APP_DIR)/MacOS
	@mkdir -p $(APP_DIR)/Frameworks
	cp $(BUILD_DIR)/aterm $(APP_DIR)/MacOS/aterm
	cp $(DYLIB) $(APP_DIR)/Frameworks/
	install_name_tool -change $(TARGET_DIR)/deps/libaterm_core.dylib @executable_path/../Frameworks/libaterm_core.dylib $(APP_DIR)/MacOS/aterm
	@mkdir -p $(dir $(APP_DIR)/Info.plist)
	@echo '<?xml version="1.0" encoding="UTF-8"?>' > $(APP_DIR)/Info.plist
	@echo '<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">' >> $(APP_DIR)/Info.plist
	@echo '<plist version="1.0">' >> $(APP_DIR)/Info.plist
	@echo '<dict>' >> $(APP_DIR)/Info.plist
	@echo '    <key>CFBundleExecutable</key>' >> $(APP_DIR)/Info.plist
	@echo '    <string>aterm</string>' >> $(APP_DIR)/Info.plist
	@echo '    <key>CFBundleIdentifier</key>' >> $(APP_DIR)/Info.plist
	@echo '    <string>com.aigentry.aterm</string>' >> $(APP_DIR)/Info.plist
	@echo '    <key>CFBundleName</key>' >> $(APP_DIR)/Info.plist
	@echo '    <string>aterm</string>' >> $(APP_DIR)/Info.plist
	@echo '    <key>CFBundleVersion</key>' >> $(APP_DIR)/Info.plist
	@echo '    <string>3.0.0</string>' >> $(APP_DIR)/Info.plist
	@echo '    <key>CFBundleShortVersionString</key>' >> $(APP_DIR)/Info.plist
	@echo '    <string>3.0</string>' >> $(APP_DIR)/Info.plist
	@echo '    <key>CFBundlePackageType</key>' >> $(APP_DIR)/Info.plist
	@echo '    <string>APPL</string>' >> $(APP_DIR)/Info.plist
	@echo '    <key>NSHighResolutionCapable</key>' >> $(APP_DIR)/Info.plist
	@echo '    <true/>' >> $(APP_DIR)/Info.plist
	@echo '    <key>CFBundleInfoDictionaryVersion</key>' >> $(APP_DIR)/Info.plist
	@echo '    <string>6.0</string>' >> $(APP_DIR)/Info.plist
	@echo '</dict>' >> $(APP_DIR)/Info.plist
	@echo '</plist>' >> $(APP_DIR)/Info.plist
	@echo "[build] App bundle created: $(BUILD_DIR)/aterm.app"

# Run
run: app
	DYLD_LIBRARY_PATH=$(APP_DIR)/Frameworks $(APP_DIR)/MacOS/aterm

# Run without .app bundle (faster for dev)
run-dev: swift
	DYLD_LIBRARY_PATH=$(TARGET_DIR) $(BUILD_DIR)/aterm

# Clean
clean:
	rm -rf $(BUILD_DIR)
	cd $(CORE_DIR) && cargo clean
